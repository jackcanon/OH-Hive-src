//! Native scheduler: periodic wake-and-reply for a Den agent, built on the `schedules_foundation`
//! schema (migration `20260920-0348-schedules-foundation`, landed 2026-09-20 as schema only --
//! this module is the first thing to write to it).
//!
//! Exists to replace what Paperclip's heartbeat was standing in for (see
//! `claude/den-paperclip-integration-decision-2026-09-24.md` in the project docs): a seat's own
//! reply comment re-triggering its own wake built a real incident on 2026-09-24 (paperclip.rs
//! now replies via a PATCH that doesn't re-wake). Owning the wake loop here removes that failure
//! class outright -- there is no external comment stream to misread, because there is no
//! external system in the loop at all. A scheduled turn is executed exactly like
//! `paperclip::handle` executes a heartbeat: one message from the conversation owner, addressed
//! to the Den agent, through the ordinary `bots_message_send` path, so every tool call still goes
//! through hub authorize/receipts and loop-safety budgets still apply.
//!
//! v1 scope, deliberately narrow:
//! - One recurrence shape: a fixed interval in seconds (`schedules.recurrence_json =
//!   {"every_secs": N}`). Cron expressions, day-of-week, etc. are a later revision of the same
//!   JSON column -- the schema does not need to change for that.
//! - One missed-run policy: skip backlog (`schedules.missed_run_policy_json =
//!   {"policy":"skip"}`). If the hub was down for three intervals, the next tick creates exactly
//!   one occurrence (fast-forwarded to the latest boundary), not three.
//! - No retries: one attempt per occurrence. A failed occurrence stays `failed`; re-running it is
//!   a human decision (`schedules_set_enabled` toggles the schedule, not a per-occurrence retry).
//! - No cross-node worker claim: the tick loop runs in-process in the hub that owns the vault, so
//!   `schedule_attempts.worker_node_id` is always NULL here. The schema allows a future
//!   multi-worker scheduler without changing this module's tables.
use super::*;
use crate::bots::*;
use std::time::{Duration, Instant};

const MIN_INTERVAL_SECS: i64 = 60;
const MAX_INTERVAL_SECS: i64 = 30 * 24 * 3600;
const RUN_TIMEOUT: Duration = Duration::from_secs(300);
/// How often `run_scheduler_tick` is called by the hub's serve loop.
pub const TICK_INTERVAL: Duration = Duration::from_secs(30);
/// Cap on occurrences executed per tick call, so one very-behind schedule cannot starve the
/// others or turn a single tick into an unbounded run.
const MAX_PER_TICK: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleSummary {
    pub id: Uuid,
    pub name: String,
    pub agent_id: Uuid,
    pub every_secs: i64,
    pub enabled: bool,
    pub paused: bool,
    pub last_materialized_through: Option<i64>,
}

struct ClaimedOccurrence {
    occurrence_id: Uuid,
    owner: UserId,
    agent_id: Uuid,
    message: String,
}

impl LocalHubStore {
    /// The single account this local hub belongs to, read from whichever paired node has
    /// confirmed one (mirrors `LocalHub::bots_owner`'s node-scoped lookup, but for CLI callers
    /// that hold the vault file directly rather than a node key -- direct file access is already
    /// a stronger trust boundary than a node key). Errors if no node has confirmed an owner yet,
    /// or if paired nodes disagree (never expected in the single-owner design this file starts
    /// with -- see `mod.rs`'s header comment -- so treated as a hard stop, not a pick-one).
    pub fn resolve_owner(&self) -> Result<UserId> {
        self.transaction(|tx| {
            let mut stmt = tx
                .prepare(
                    "SELECT DISTINCT owner_member_id FROM nodes WHERE owner_member_id IS NOT NULL",
                )
                .map_err(db_error)?;
            let owners: Vec<String> = stmt
                .query_map([], |r| r.get(0))
                .map_err(db_error)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_error)?;
            match owners.as_slice() {
                [one] => Uuid::parse_str(one).map_err(|_| rejected("stored owner id is invalid")),
                [] => Err(rejected(
                    "no node has confirmed a Hive account owner yet; open Bots once while online",
                )),
                _ => Err(rejected("paired nodes disagree on account owner")),
            }
        })
    }

    /// Create a schedule and its first (only, for now) revision in one step. The revision is
    /// implicitly authorized by the caller being the owner -- there is no separate approval step
    /// in v1, matching every other owner-issued command in this store (e.g. `set_hub_name`).
    /// `every_secs` bounds match the handoff doc's examples ("every 4 hours") at the low end and
    /// a sanity ceiling at the high end; nothing here stops a much longer interval from being a
    /// good idea, the ceiling just keeps a fat-fingered value from silently meaning "never".
    ///
    /// `start_at`: when to fire the *first* occurrence. `None` keeps the original v1 behavior --
    /// one interval from creation. `Some(ts)` anchors the first occurrence to that exact instant
    /// instead, which every later occurrence then stays locked to (every_secs later, forever) --
    /// this is what a "twice a week, fixed local time" schedule needs, since v1 has no cron/
    /// day-of-week recurrence shape yet (see the module doc). `ts` in the past is accepted, not
    /// rejected: the next tick just fires it right away, fast-forwarded like any other overdue
    /// occurrence under the "skip" missed-run policy -- there is nothing unsafe about it, so
    /// there is no reason to make the caller special-case "start now" separately from "start at
    /// a specific past-due instant".
    pub fn schedules_create(
        &self,
        owner: UserId,
        name: &str,
        agent_id: Uuid,
        message: &str,
        every_secs: i64,
        start_at: Option<i64>,
    ) -> Result<Uuid> {
        check_text(name, 200)?;
        check_text(message, 8_000)?;
        if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&every_secs) {
            return Err(rejected(
                "schedule interval must be between 60 seconds and 30 days",
            ));
        }
        let schedule_id = Uuid::new_v4();
        let auth_id = Uuid::new_v4();
        let rev_id = Uuid::new_v4();
        let n = now();
        // last_materialized_through is the baseline the materializer adds every_secs to, to get
        // the next due instant. Backdating it by one interval from the requested start makes that
        // arithmetic land the first occurrence exactly on start_at, with no special-casing in the
        // materializer itself.
        let initial_baseline = start_at.map(|t| t - every_secs).unwrap_or(n);
        let owned_name = name.to_owned();
        let owned_message = message.to_owned();
        self.transaction(move |tx| {
            let agent_owner: Option<String> = tx
                .query_row(
                    "SELECT owner FROM agent_profiles WHERE id=?1 AND archived=0",
                    [agent_id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if agent_owner.as_deref() != Some(&owner.to_string()) {
                return Err(rejected("no such agent for this account"));
            }
            tx.execute(
                "INSERT INTO schedules(id,owner,name,current_revision,recurrence_json,\
                 missed_run_policy_json,enabled,paused,last_materialized_through,created_at,\
                 updated_at) VALUES(?1,?2,?3,?4,?5,?6,1,0,?7,?8,?8)",
                params![
                    schedule_id.to_string(),
                    owner.to_string(),
                    owned_name,
                    rev_id.to_string(),
                    json!({"every_secs": every_secs}).to_string(),
                    json!({"policy": "skip"}).to_string(),
                    initial_baseline, // + every_secs = the first due instant (see doc comment)
                    n,
                ],
            )
            .map_err(db_error)?;
            tx.execute(
                "INSERT INTO schedule_authorizations(id,schedule_id,owner,scope_json,\
                 revision_digest,approved_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    auth_id.to_string(),
                    schedule_id.to_string(),
                    owner.to_string(),
                    json!({"kind": "owner_created"}).to_string(),
                    digest(&owned_message),
                    n,
                ],
            )
            .map_err(db_error)?;
            tx.execute(
                "INSERT INTO schedule_revisions(id,schedule_id,revision_number,task_spec_json,\
                 agent_id,host_policy_json,model_policy_json,resource_scope_json,budgets_json,\
                 output_destination_json,authorization_id,created_at) \
                 VALUES(?1,?2,1,?3,?4,'{}',NULL,'{}','{}',NULL,?5,?6)",
                params![
                    rev_id.to_string(),
                    schedule_id.to_string(),
                    json!({"message": owned_message}).to_string(),
                    agent_id.to_string(),
                    auth_id.to_string(),
                    n,
                ],
            )
            .map_err(db_error)?;
            Ok(schedule_id)
        })
    }

    pub fn schedules_list(&self, owner: UserId) -> Result<Vec<ScheduleSummary>> {
        self.transaction(|tx| {
            let mut stmt = tx
                .prepare(
                    "SELECT s.id,s.name,r.agent_id,s.recurrence_json,s.enabled,s.paused,\
                     s.last_materialized_through \
                     FROM schedules s JOIN schedule_revisions r ON r.id=s.current_revision \
                     WHERE s.owner=?1 ORDER BY s.created_at",
                )
                .map_err(db_error)?;
            let rows = stmt
                .query_map([owner.to_string()], |r| {
                    let recurrence: String = r.get(3)?;
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        recurrence,
                        r.get::<_, i64>(4)? != 0,
                        r.get::<_, i64>(5)? != 0,
                        r.get::<_, Option<i64>>(6)?,
                    ))
                })
                .map_err(db_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_error)?;
            rows.into_iter()
                .map(|(id, name, agent, recurrence, enabled, paused, last)| {
                    let every_secs = decode::<Value>(&recurrence)?
                        .get("every_secs")
                        .and_then(Value::as_i64)
                        .ok_or_else(|| rejected("stored recurrence is invalid"))?;
                    Ok(ScheduleSummary {
                        id: id
                            .parse()
                            .map_err(|_| rejected("stored schedule id is invalid"))?,
                        name,
                        agent_id: agent
                            .parse()
                            .map_err(|_| rejected("stored agent id is invalid"))?,
                        every_secs,
                        enabled,
                        paused,
                        last_materialized_through: last,
                    })
                })
                .collect()
        })
    }

    pub fn schedules_set_enabled(&self, owner: UserId, id: Uuid, enabled: bool) -> Result<()> {
        self.transaction(move |tx| {
            let n = tx
                .execute(
                    "UPDATE schedules SET enabled=?1,updated_at=?2 WHERE id=?3 AND owner=?4",
                    params![enabled as i64, now(), id.to_string(), owner.to_string()],
                )
                .map_err(db_error)?;
            if n == 0 {
                return Err(rejected("no such schedule for this account"));
            }
            Ok(())
        })
    }

    /// For every enabled, unpaused schedule that is due, create at most one occurrence and
    /// advance `last_materialized_through` to that occurrence's boundary. "Skip" policy: a
    /// schedule that missed several intervals is fast-forwarded to the latest boundary at or
    /// before `now` rather than backfilling a burst of occurrences.
    fn schedules_materialize_due(&self, now_ts: i64) -> Result<usize> {
        self.transaction(move |tx| {
            let mut stmt = tx
                .prepare(
                    "SELECT id,current_revision,recurrence_json,last_materialized_through,\
                     created_at FROM schedules WHERE enabled=1 AND paused=0",
                )
                .map_err(db_error)?;
            let due: Vec<(String, String, String, i64)> = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<i64>>(3)?.unwrap_or(r.get::<_, i64>(4)?),
                    ))
                })
                .map_err(db_error)?
                .collect::<rusqlite::Result<_>>()
                .map_err(db_error)?;
            drop(stmt);
            let mut created = 0usize;
            for (schedule_id, revision_id, recurrence, last) in due {
                let every_secs = decode::<Value>(&recurrence)?
                    .get("every_secs")
                    .and_then(Value::as_i64)
                    .filter(|s| *s > 0)
                    .ok_or_else(|| rejected("stored recurrence is invalid"))?;
                let mut next = last + every_secs;
                if next > now_ts {
                    continue;
                }
                while next + every_secs <= now_ts {
                    next += every_secs;
                }
                let occurrence_id = Uuid::new_v4();
                let request_id = format!("sched:{schedule_id}:{next}");
                tx.execute(
                    "INSERT INTO schedule_occurrences(id,schedule_id,revision_id,due_at,state,\
                     reason,request_id,linked_delivery_ref,created_at,updated_at) \
                     VALUES(?1,?2,?3,?4,'queued',NULL,?5,NULL,?6,?6) \
                     ON CONFLICT(request_id) DO NOTHING",
                    params![
                        occurrence_id.to_string(),
                        schedule_id,
                        revision_id,
                        next,
                        request_id,
                        now_ts,
                    ],
                )
                .map_err(db_error)?;
                tx.execute(
                    "UPDATE schedules SET last_materialized_through=?1,updated_at=?2 WHERE id=?3",
                    params![next, now_ts, schedule_id],
                )
                .map_err(db_error)?;
                created += 1;
            }
            Ok(created)
        })
    }

    /// Atomically claim one due, still-queued occurrence. `state='queued'` in the `UPDATE`'s
    /// `WHERE` is the claim: a second caller racing for the same row updates zero rows and moves
    /// on, the same optimistic-claim shape leases use elsewhere in this store.
    fn schedules_claim_due_occurrence(&self, now_ts: i64) -> Result<Option<ClaimedOccurrence>> {
        self.transaction(move |tx| {
            let found: Option<(String, String, String)> = tx
                .query_row(
                    "SELECT id,schedule_id,revision_id FROM schedule_occurrences \
                     WHERE state='queued' AND due_at<=?1 ORDER BY due_at LIMIT 1",
                    [now_ts],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(db_error)?;
            let Some((occ_id, _schedule_id, revision_id)) = found else {
                return Ok(None);
            };
            let claimed = tx
                .execute(
                    "UPDATE schedule_occurrences SET state='running',updated_at=?1 \
                     WHERE id=?2 AND state='queued'",
                    params![now_ts, occ_id],
                )
                .map_err(db_error)?;
            if claimed == 0 {
                return Ok(None);
            }
            let (owner, agent_id, task_spec): (String, String, String) = tx
                .query_row(
                    "SELECT s.owner,r.agent_id,r.task_spec_json FROM schedule_revisions r \
                     JOIN schedules s ON s.id=r.schedule_id WHERE r.id=?1",
                    [&revision_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(db_error)?;
            let message = decode::<Value>(&task_spec)?
                .get("message")
                .and_then(Value::as_str)
                .ok_or_else(|| rejected("stored task spec is invalid"))?
                .to_owned();
            Ok(Some(ClaimedOccurrence {
                occurrence_id: occ_id.parse().map_err(|_| rejected("bad occurrence id"))?,
                owner: owner.parse().map_err(|_| rejected("bad owner id"))?,
                agent_id: agent_id.parse().map_err(|_| rejected("bad agent id"))?,
                message,
            }))
        })
    }

    fn schedules_complete_occurrence(
        &self,
        occurrence_id: Uuid,
        succeeded: bool,
        result_ref: Option<&str>,
        failure_class: Option<&str>,
    ) -> Result<()> {
        let n = now();
        self.transaction(move |tx| {
            tx.execute(
                "UPDATE schedule_occurrences SET state=?1,updated_at=?2 WHERE id=?3",
                params![
                    if succeeded { "succeeded" } else { "failed" },
                    n,
                    occurrence_id.to_string(),
                ],
            )
            .map_err(db_error)?;
            let attempt_id = Uuid::new_v4();
            tx.execute(
                "INSERT INTO schedule_attempts(id,occurrence_id,attempt_number,worker_node_id,\
                 lease_generation,started_at,ended_at,failure_class,result_ref,usage_json,\
                 created_at) VALUES(?1,?2,1,NULL,0,?3,?3,?4,?5,NULL,?3)",
                params![
                    attempt_id.to_string(),
                    occurrence_id.to_string(),
                    n,
                    failure_class,
                    result_ref,
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }
}

/// Find the owner's existing DM with this agent, or start one. Mirrors how a paperclip seat's
/// conversation was set up by hand (`conversation_id` in `paperclip-seats.json`); a schedule
/// does the same lookup itself instead of requiring one more thing the caller has to wire up.
///
/// `pub(crate)`: also used by `bots::bots_handoff_create_and_wake`/`bots_handoff_resolve`, which
/// wake an agent (and later notify the handoff's source agent) the same way a schedule does --
/// one lookup, not two competing ones.
pub(crate) fn find_or_create_agent_dm(
    store: &LocalHubStore,
    owner: UserId,
    agent_id: Uuid,
) -> Result<Uuid> {
    let existing = store.bots_conversations_list(Principal::User(owner))?;
    if let Some(c) = existing
        .iter()
        .find(|c| c.kind == ConversationKind::AgentDm && c.coordinator == Some(agent_id))
    {
        return Ok(c.id);
    }
    let c = store.bots_conversations_create(NewConversation {
        title: None,
        owner,
        kind: ConversationKind::AgentDm,
        project_id: None,
        coordinator: Some(agent_id),
        storage_scope: StorageScope::LocalOnly,
    })?;
    Ok(c.id)
}

/// Send the scheduled message and wait (up to `RUN_TIMEOUT`) for the agent's reply, the same
/// send-then-poll shape `paperclip::handle` uses for a heartbeat. Returns the reply body, or
/// `Err` with a short failure class for `schedule_attempts.failure_class`.
async fn run_occurrence(
    store: LocalHubStore,
    occ: ClaimedOccurrence,
) -> Result<String, &'static str> {
    let conv = tokio::task::spawn_blocking({
        let store = store.clone();
        move || find_or_create_agent_dm(&store, occ.owner, occ.agent_id)
    })
    .await
    .map_err(|_| "hub task failed")?
    .map_err(|_| "could not open agent conversation")?;

    let s = store.clone();
    let (owner, den, key) = (
        occ.owner,
        occ.agent_id,
        format!("sched:{}", occ.occurrence_id),
    );
    let text = format!(
        "Scheduled task.\n\n{}\n\nReply with your response; it will be recorded as this run's result.",
        occ.message
    );
    let sent = tokio::task::spawn_blocking(move || {
        let c = s.bots_conversation_get(conv)?;
        s.bots_message_send(
            Principal::User(owner),
            conv,
            key,
            c.policy_revision,
            vec![den],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some(text),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
    })
    .await
    .map_err(|_| "hub task failed")?
    .map_err(|_| "hub rejected the message")?;

    let deadline = Instant::now() + RUN_TIMEOUT;
    let mut after = sent.server_sequence;
    loop {
        let s = store.clone();
        let page = tokio::task::spawn_blocking(move || {
            s.bots_messages_list(
                Principal::User(owner),
                conv,
                MessagePage {
                    before: None,
                    after: Some(after),
                    limit: 100,
                },
            )
        })
        .await
        .map_err(|_| "hub task failed")?
        .map_err(|_| "hub read failed")?;
        if let Some(m) = page.iter().find(|m| {
            m.author == Principal::Agent(den)
                && m.kind == MessageKind::Text
                && m.body.as_deref().is_some_and(|b| !b.trim().is_empty())
        }) {
            return Ok(m.body.clone().unwrap_or_default());
        }
        if let Some(last) = page.last() {
            after = last.server_sequence;
        }
        if Instant::now() >= deadline {
            return Err("no agent reply in time");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// One pass: materialize due occurrences, then execute up to `MAX_PER_TICK` of them in this
/// process, sequentially (v1 has no need for concurrency here -- a schedule interval is measured
/// in hours, not seconds). Called on a loop by the hub's serve setup; also callable directly in
/// tests. Never panics on a single occurrence's failure -- one bad run does not stop the tick.
pub async fn run_scheduler_tick(store: LocalHubStore) -> Result<usize> {
    let n = now();
    let s = store.clone();
    tokio::task::spawn_blocking(move || s.schedules_materialize_due(n))
        .await
        .map_err(|_| rejected("scheduler materialize task failed"))??;

    let mut executed = 0;
    while executed < MAX_PER_TICK {
        let s = store.clone();
        let claimed = tokio::task::spawn_blocking(move || s.schedules_claim_due_occurrence(now()))
            .await
            .map_err(|_| rejected("scheduler claim task failed"))??;
        let Some(occ) = claimed else { break };
        let occurrence_id = occ.occurrence_id;
        let result = run_occurrence(store.clone(), occ).await;
        let s = store.clone();
        let complete = match &result {
            Ok(body) => {
                let body = body.clone();
                tokio::task::spawn_blocking(move || {
                    s.schedules_complete_occurrence(occurrence_id, true, Some(&body), None)
                })
            }
            Err(class) => {
                let class = *class;
                tokio::task::spawn_blocking(move || {
                    s.schedules_complete_occurrence(occurrence_id, false, None, Some(class))
                })
            }
        };
        complete
            .await
            .map_err(|_| rejected("scheduler complete task failed"))??;
        executed += 1;
    }
    Ok(executed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(store: &LocalHubStore, owner: UserId) -> Uuid {
        store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Scheduled".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(Uuid::new_v4()),
                capability_policy_ref: "default".into(),
                provider_account_ref: None,
                memory_namespace: "sched".into(),
            })
            .unwrap()
            .id
    }

    fn owned_store() -> (LocalHubStore, UserId) {
        let store = LocalHubStore::in_memory().unwrap();
        let owner = Uuid::new_v4();
        let node = Uuid::new_v4();
        store
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO nodes(id,name,owner_member_id) VALUES(?1,'test-node',?2) \
                     ON CONFLICT(id) DO UPDATE SET owner_member_id=excluded.owner_member_id",
                    params![node.to_string(), owner.to_string()],
                )
                .map_err(db_error)?;
                Ok(())
            })
            .unwrap();
        (store, owner)
    }

    #[test]
    fn create_validates_interval_and_agent_ownership() {
        let (store, owner) = owned_store();
        let a = agent(&store, owner);
        assert!(
            store
                .schedules_create(owner, "x", a, "hi", 30, None)
                .is_err(),
            "below minimum"
        );
        assert!(
            store
                .schedules_create(owner, "x", a, "hi", 31 * 24 * 3600, None)
                .is_err(),
            "above maximum"
        );
        assert!(
            store
                .schedules_create(owner, "x", Uuid::new_v4(), "hi", 3600, None)
                .is_err(),
            "unknown agent"
        );
        let other_owner = Uuid::new_v4();
        assert!(
            store
                .schedules_create(other_owner, "x", a, "hi", 3600, None)
                .is_err(),
            "agent belongs to a different account"
        );
        let id = store
            .schedules_create(owner, "Ladder scan", a, "run it", 3600, None)
            .unwrap();
        let list = store.schedules_list(owner).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].every_secs, 3600);
        assert!(list[0].enabled);
        assert!(!list[0].paused);
    }

    #[test]
    fn resolve_owner_requires_exactly_one_confirmed_owner() {
        let store = LocalHubStore::in_memory().unwrap();
        assert!(store.resolve_owner().is_err(), "no owner confirmed yet");
        let (store, owner) = owned_store();
        assert_eq!(store.resolve_owner().unwrap(), owner);
    }

    #[test]
    fn materialize_is_idempotent_and_respects_the_interval() {
        let (store, owner) = owned_store();
        let a = agent(&store, owner);
        store
            .schedules_create(owner, "x", a, "hi", 3600, None)
            .unwrap();
        // Not due yet: nothing to do right after creation.
        assert_eq!(store.schedules_materialize_due(now()).unwrap(), 0);
        // Fast-forward past several missed intervals: exactly one occurrence, "skip" policy.
        let far_future = now() + 10 * 3600;
        assert_eq!(store.schedules_materialize_due(far_future).unwrap(), 1);
        assert_eq!(
            store.schedules_materialize_due(far_future).unwrap(),
            0,
            "not due again yet"
        );
        let claimed = store.schedules_claim_due_occurrence(far_future).unwrap();
        assert!(claimed.is_some());
        assert!(
            store
                .schedules_claim_due_occurrence(far_future)
                .unwrap()
                .is_none(),
            "the one due occurrence was already claimed"
        );
    }

    #[test]
    fn start_at_anchors_the_first_occurrence_to_an_exact_instant() {
        let (store, owner) = owned_store();
        let a = agent(&store, owner);
        let anchor = now() + 3 * 24 * 3600; // "in 3 days", standing in for e.g. next Sunday 6am
        store
            .schedules_create(owner, "x", a, "hi", 7 * 24 * 3600, Some(anchor))
            .unwrap();
        // Not due at all before the anchor, no matter how close -- unlike the None case, this
        // schedule's first occurrence isn't "one interval from creation", it's exactly `anchor`.
        assert_eq!(
            store.schedules_materialize_due(anchor - 1).unwrap(),
            0,
            "must not fire even one second before the anchor"
        );
        // Due exactly at the anchor instant.
        assert_eq!(store.schedules_materialize_due(anchor).unwrap(), 1);
        // Every later occurrence stays locked to the anchor plus whole intervals: the next one is
        // due in exactly 7 days from the anchor, not 7 days from whenever `now()` happened to be
        // when the schedule was created.
        assert_eq!(
            store
                .schedules_materialize_due(anchor + 7 * 24 * 3600 - 1)
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .schedules_materialize_due(anchor + 7 * 24 * 3600)
                .unwrap(),
            1
        );
    }

    #[test]
    fn start_at_in_the_past_fires_on_the_next_tick_like_any_other_overdue_occurrence() {
        let (store, owner) = owned_store();
        let a = agent(&store, owner);
        let past = now() - 60;
        store
            .schedules_create(owner, "x", a, "hi", 3600, Some(past))
            .unwrap();
        assert_eq!(
            store.schedules_materialize_due(now()).unwrap(),
            1,
            "a start_at already in the past is due immediately, same as any other missed run"
        );
    }

    #[test]
    fn disabled_and_paused_schedules_are_not_materialized() {
        let (store, owner) = owned_store();
        let a = agent(&store, owner);
        let id = store
            .schedules_create(owner, "x", a, "hi", 3600, None)
            .unwrap();
        store.schedules_set_enabled(owner, id, false).unwrap();
        assert_eq!(
            store.schedules_materialize_due(now() + 10 * 3600).unwrap(),
            0
        );
        store.schedules_set_enabled(owner, id, true).unwrap();
        assert_eq!(
            store.schedules_materialize_due(now() + 10 * 3600).unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn tick_runs_a_due_occurrence_end_to_end() {
        let (store, owner) = owned_store();
        let a = agent(&store, owner);
        store
            .schedules_create(owner, "Ladder scan", a, "scan it", 60, None)
            .unwrap();

        // Simulate the agent host: answer whatever DM shows up, once.
        let s = store.clone();
        let responder = tokio::spawn(async move {
            let conv = loop {
                let convs = s.bots_conversations_list(Principal::User(owner)).unwrap();
                if let Some(c) = convs.iter().find(|c| c.coordinator == Some(a)) {
                    break c.id;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            };
            loop {
                let msgs = s
                    .bots_messages_list(
                        Principal::User(owner),
                        conv,
                        MessagePage {
                            before: None,
                            after: None,
                            limit: 50,
                        },
                    )
                    .unwrap();
                if let Some(owner_msg) = msgs.iter().find(|m| m.author == Principal::User(owner)) {
                    let c = s.bots_conversation_get(conv).unwrap();
                    s.bots_message_send(
                        Principal::Agent(a),
                        conv,
                        "reply-1".into(),
                        c.policy_revision,
                        vec![],
                        NewMessage {
                            thread_root: None,
                            kind: MessageKind::Text,
                            body: Some("scan complete".into()),
                            attachment_refs: vec![],
                            task_ref: None,
                            turn_ref: None,
                            source_event_ref: None,
                        },
                    )
                    .unwrap();
                    let _ = owner_msg;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });

        // Force the schedule to be due right now instead of waiting out a real interval, then
        // pull the occurrence's due_at back to real "now" -- `run_scheduler_tick` claims with
        // its own real-time `now()`, not the future timestamp used to force materialization.
        store.schedules_materialize_due(now() + 3600).unwrap();
        store
            .transaction(|tx| {
                tx.execute("UPDATE schedule_occurrences SET due_at=?1", [now()])
                    .map_err(db_error)
            })
            .unwrap();
        let executed = run_scheduler_tick(store.clone()).await.unwrap();
        responder.await.unwrap();
        assert_eq!(executed, 1);

        let list = store.schedules_list(owner).unwrap();
        assert!(list[0].last_materialized_through.is_some());
        let occ_state: String = store
            .transaction(|tx| {
                tx.query_row(
                    "SELECT state FROM schedule_occurrences ORDER BY created_at DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        assert_eq!(occ_state, "succeeded");
        let result: Option<String> = store
            .transaction(|tx| {
                tx.query_row(
                    "SELECT result_ref FROM schedule_attempts ORDER BY created_at DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        assert_eq!(result.as_deref(), Some("scan complete"));
    }

    #[tokio::test]
    async fn tick_marks_a_timed_out_occurrence_failed() {
        let (store, owner) = owned_store();
        let a = agent(&store, owner);
        store
            .schedules_create(owner, "x", a, "hi", 60, None)
            .unwrap();
        store.schedules_materialize_due(now() + 3600).unwrap();
        // No responder: the claimed occurrence will time out. RUN_TIMEOUT is 300s in real use;
        // this test does not wait that long -- it only checks the claim/complete bookkeeping
        // works, using the sync half directly instead of the real timeout.
        let claimed = store
            .schedules_claim_due_occurrence(now() + 3600)
            .unwrap()
            .unwrap();
        store
            .schedules_complete_occurrence(
                claimed.occurrence_id,
                false,
                None,
                Some("no agent reply in time"),
            )
            .unwrap();
        let occ_state: String = store
            .transaction(|tx| {
                tx.query_row(
                    "SELECT state FROM schedule_occurrences ORDER BY created_at DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)
            })
            .unwrap();
        assert_eq!(occ_state, "failed");
    }

    // Coordinator -> Coder hand-off (Cmd Work "Build Coordinator -> Coder agent hand-off"),
    // colocated here because it reuses this module's `find_or_create_agent_dm` and its test
    // helpers rather than any new plumbing of its own.

    fn new_handoff(source: Uuid, target: Uuid) -> NewHandoff {
        NewHandoff {
            source_agent: source,
            target_agent: target,
            project_id: None,
            task_or_question: "add a --start-at flag".into(),
            acceptance_criteria: "cargo test passes and the flag anchors the first occurrence"
                .into(),
            artifact_refs: vec![],
            allowed_tools: vec![],
            parent_run: None,
            reply_to_thread: None,
            budgets: None,
            deadline: chrono::Utc::now() + chrono::Duration::hours(24),
        }
    }

    #[test]
    fn handoff_create_and_wake_delivers_the_task_into_the_targets_dm() {
        let (store, owner) = owned_store();
        let coordinator = agent(&store, owner);
        let coder = agent(&store, owner);
        let handoff = store
            .bots_handoff_create_and_wake(owner, new_handoff(coordinator, coder))
            .unwrap();
        assert_eq!(handoff.state, HandoffState::Requested);
        assert!(handoff.receipt.is_none());

        let conv = find_or_create_agent_dm(&store, owner, coder).unwrap();
        let messages = store
            .bots_messages_list(
                Principal::User(owner),
                conv,
                MessagePage {
                    before: None,
                    after: None,
                    limit: 10,
                },
            )
            .unwrap();
        let sent = messages.last().expect("wake message was sent");
        let body = sent.body.as_deref().unwrap_or_default();
        assert!(body.contains(&handoff.id.to_string()));
        assert!(body.contains("add a --start-at flag"));

        // Re-opening the same DM (as `find_or_create_agent_dm` does for every schedule tick and
        // every handoff on this agent) must not spawn a second conversation.
        let conv_again = find_or_create_agent_dm(&store, owner, coder).unwrap();
        assert_eq!(conv, conv_again);
    }

    #[test]
    fn handoff_resolve_writes_the_receipt_and_notifies_the_source_agent() {
        let (store, owner) = owned_store();
        let coordinator = agent(&store, owner);
        let coder = agent(&store, owner);
        let handoff = store
            .bots_handoff_create_and_wake(owner, new_handoff(coordinator, coder))
            .unwrap();

        let resolved = store
            .bots_handoff_resolve(
                owner,
                handoff.id,
                HandoffState::Completed,
                "shipped as PR #50".into(),
                vec!["https://github.com/jackcanon/OH-Hive-src/pull/50".into()],
            )
            .unwrap();
        assert_eq!(resolved.state, HandoffState::Completed);
        let receipt = resolved.receipt.expect("receipt was written");
        assert_eq!(receipt.state, HandoffState::Completed);
        assert_eq!(receipt.summary, "shipped as PR #50");

        let conv = find_or_create_agent_dm(&store, owner, coordinator).unwrap();
        let messages = store
            .bots_messages_list(
                Principal::User(owner),
                conv,
                MessagePage {
                    before: None,
                    after: None,
                    limit: 10,
                },
            )
            .unwrap();
        let receipt_msg = messages.last().expect("receipt message was sent");
        assert_eq!(receipt_msg.kind, MessageKind::TaskReceipt);
        assert!(receipt_msg
            .body
            .as_deref()
            .unwrap_or_default()
            .contains("shipped as PR #50"));

        // A resolved handoff cannot be resolved a second time -- one receipt, not a moving one.
        assert!(store
            .bots_handoff_resolve(
                owner,
                handoff.id,
                HandoffState::Failed,
                "changed my mind".into(),
                vec![],
            )
            .is_err());
    }

    #[test]
    fn handoff_resolve_rejects_a_non_terminal_state() {
        let (store, owner) = owned_store();
        let coordinator = agent(&store, owner);
        let coder = agent(&store, owner);
        let handoff = store
            .bots_handoff_create_and_wake(owner, new_handoff(coordinator, coder))
            .unwrap();
        assert!(store
            .bots_handoff_resolve(
                owner,
                handoff.id,
                HandoffState::InProgress,
                "".into(),
                vec![]
            )
            .is_err());
    }
}
