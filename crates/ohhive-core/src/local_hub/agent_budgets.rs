//! Per-agent monthly token usage and optional caps. See
//! den-paperclip-integration-decision-2026-09-24.md: "Per-agent monthly budget next to per-card
//! metered spend -- now the next priority. Needed either way, low risk, do it natively rather
//! than trusting Paperclip's cost reporting." v1 scope is record + report only -- exceeding a
//! budget does not block a turn, it only shows up in `bots hub budget status`.
//!
//! Token-based, not USD: usage recorded here is exactly what a `TurnUsage` reports from
//! whichever runner ran the turn, no per-provider price table to keep in sync or trust -- the
//! same reasoning as the decision doc's Paperclip-cost distrust, applied to every locally-run
//! turn (local model, library-tool, and cloud/BYOK alike -- `TurnUsage` is produced uniformly by
//! all three).
//!
//! Recording only ever happens on the machine that actually ran the turn:
//! `DeliveryStore::record_agent_usage`'s `LocalHubStore` override is the only place a row is
//! written; `RemoteLocalHub` keeps the trait's no-op default. A fleet-hosted agent's usage is
//! not tracked here in v1 -- the same narrowing the native scheduler already has for cross-node
//! worker claims.
use super::*;
use rusqlite::OptionalExtension;

/// One agent's usage and (if set) budget for the current UTC calendar month.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentBudgetStatus {
    pub agent: Uuid,
    pub period: String, // "YYYY-MM", UTC
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub monthly_token_limit: Option<i64>,
}
impl AgentBudgetStatus {
    pub fn total_tokens(&self) -> i64 {
        self.prompt_tokens + self.completion_tokens
    }
    pub fn over_budget(&self) -> bool {
        self.monthly_token_limit
            .map(|limit| self.total_tokens() >= limit)
            .unwrap_or(false)
    }
}

/// UTC "YYYY-MM" for `now()`'s timestamp -- the period key `agent_usage_periods` rows over.
fn current_period() -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(now(), 0)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m")
        .to_string()
}

fn owned_agent(tx: &Transaction<'_>, agent: Uuid) -> Result<()> {
    let ok: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE id=?1)",
            [agent.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !ok {
        return Err(rejected("agent not found"));
    }
    Ok(())
}

impl LocalHubStore {
    /// Adds `usage` to `agent`'s running total for the current UTC month. Called once per
    /// completed turn by `DeliveryExecutor` (via `DeliveryStore::record_agent_usage`);
    /// best-effort by design -- a failure here does not fail the turn or the reply that was
    /// already sent, it just means this one turn's tokens are missing from the report.
    pub(crate) fn bots_agent_usage_record(
        &self,
        agent: Uuid,
        usage: crate::bots::TurnUsage,
    ) -> Result<()> {
        let period = current_period();
        self.transaction(|tx| {
            let t = now();
            tx.execute(
                "INSERT INTO agent_usage_periods(agent,period,prompt_tokens,completion_tokens,updated_at) \
                 VALUES(?1,?2,?3,?4,?5) \
                 ON CONFLICT(agent,period) DO UPDATE SET \
                   prompt_tokens=prompt_tokens+excluded.prompt_tokens, \
                   completion_tokens=completion_tokens+excluded.completion_tokens, \
                   updated_at=excluded.updated_at",
                params![
                    agent.to_string(),
                    period,
                    usage.prompt_tokens as i64,
                    usage.completion_tokens as i64,
                    t
                ],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Sets (or replaces) `agent`'s monthly token cap. CLI-only local path, same reasoning as
    /// `agent_tools.rs::owned_agent`'s doc comment: the hub-serving machine's HOME never has a
    /// paired node key, so this resolves ownership from the local `agent_profiles` table rather
    /// than `with_node` authentication.
    pub fn bots_agent_budget_set_local(&self, agent: Uuid, monthly_token_limit: i64) -> Result<()> {
        if monthly_token_limit <= 0 {
            return Err(rejected("monthly token limit must be positive"));
        }
        self.transaction(|tx| {
            owned_agent(tx, agent)?;
            let t = now();
            tx.execute(
                "INSERT INTO agent_budgets(agent,monthly_token_limit,created_at,updated_at) \
                 VALUES(?1,?2,?3,?3) \
                 ON CONFLICT(agent) DO UPDATE SET monthly_token_limit=excluded.monthly_token_limit, updated_at=excluded.updated_at",
                params![agent.to_string(), monthly_token_limit, t],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// Clears any monthly cap set for `agent` (usage tracking keeps recording either way).
    pub fn bots_agent_budget_clear_local(&self, agent: Uuid) -> Result<()> {
        self.transaction(|tx| {
            owned_agent(tx, agent)?;
            tx.execute(
                "DELETE FROM agent_budgets WHERE agent=?1",
                [agent.to_string()],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }

    /// This agent's usage for the current UTC month, plus its cap if one is set.
    pub fn bots_agent_budget_status_local(&self, agent: Uuid) -> Result<AgentBudgetStatus> {
        let period = current_period();
        self.transaction(|tx| {
            owned_agent(tx, agent)?;
            let (prompt_tokens, completion_tokens): (i64, i64) = tx
                .query_row(
                    "SELECT prompt_tokens,completion_tokens FROM agent_usage_periods WHERE agent=?1 AND period=?2",
                    params![agent.to_string(), period],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(db_error)?
                .unwrap_or((0, 0));
            let monthly_token_limit: Option<i64> = tx
                .query_row(
                    "SELECT monthly_token_limit FROM agent_budgets WHERE agent=?1",
                    [agent.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            Ok(AgentBudgetStatus {
                agent,
                period: period.clone(),
                prompt_tokens,
                completion_tokens,
                monthly_token_limit,
            })
        })
    }

    /// Status for every agent that either has a cap set or has recorded usage this month --
    /// agents nobody has touched yet (no budget, no turns run) are omitted rather than listed
    /// at all-zero, which would just be noise as the fleet grows.
    pub fn bots_agent_budget_list_local(&self) -> Result<Vec<AgentBudgetStatus>> {
        let period = current_period();
        self.transaction(|tx| {
            let mut stmt = tx
                .prepare(
                    "SELECT a.id, \
                            COALESCE(u.prompt_tokens,0), COALESCE(u.completion_tokens,0), \
                            b.monthly_token_limit \
                     FROM agent_profiles a \
                     LEFT JOIN agent_usage_periods u ON u.agent=a.id AND u.period=?1 \
                     LEFT JOIN agent_budgets b ON b.agent=a.id \
                     WHERE u.agent IS NOT NULL OR b.agent IS NOT NULL \
                     ORDER BY a.id",
                )
                .map_err(db_error)?;
            let period_for_rows = period.clone();
            let rows = stmt
                .query_map(params![period], move |r| {
                    let agent: String = r.get(0)?;
                    Ok(AgentBudgetStatus {
                        agent: Uuid::parse_str(&agent).unwrap_or_default(),
                        period: period_for_rows.clone(),
                        prompt_tokens: r.get(1)?,
                        completion_tokens: r.get(2)?,
                        monthly_token_limit: r.get(3)?,
                    })
                })
                .map_err(db_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(db_error)?;
            Ok(rows)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::{AgentRuntimeKind, NewAgentProfile, TurnUsage};

    fn make_agent(store: &LocalHubStore) -> Uuid {
        let credentials = store.enroll_owner("host").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(credentials.node_id, owner).unwrap();
        store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Budget test agent".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(credentials.node_id),
                capability_policy_ref: "no-tools".into(),
                provider_account_ref: None,
                memory_namespace: "budget-test".into(),
            })
            .unwrap()
            .id
    }

    #[test]
    fn usage_accumulates_across_turns_in_the_current_period_and_is_isolated_per_agent() {
        let store = LocalHubStore::in_memory().unwrap();
        let a = make_agent(&store);
        let b = make_agent(&store);
        store
            .bots_agent_usage_record(
                a,
                TurnUsage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                },
            )
            .unwrap();
        store
            .bots_agent_usage_record(
                a,
                TurnUsage {
                    prompt_tokens: 50,
                    completion_tokens: 5,
                },
            )
            .unwrap();
        let status = store.bots_agent_budget_status_local(a).unwrap();
        assert_eq!(status.prompt_tokens, 150);
        assert_eq!(status.completion_tokens, 25);
        assert_eq!(status.total_tokens(), 175);
        assert_eq!(status.period, current_period());
        assert!(status.monthly_token_limit.is_none());
        assert!(!status.over_budget());

        // Agent b never had a turn recorded -- its own status starts at zero, not b's numbers
        // contaminated by a's.
        let status_b = store.bots_agent_budget_status_local(b).unwrap();
        assert_eq!(status_b.total_tokens(), 0);
    }

    #[test]
    fn set_clear_and_over_budget_detection() {
        let store = LocalHubStore::in_memory().unwrap();
        let a = make_agent(&store);
        store.bots_agent_budget_set_local(a, 100).unwrap();
        let status = store.bots_agent_budget_status_local(a).unwrap();
        assert_eq!(status.monthly_token_limit, Some(100));
        assert!(!status.over_budget());

        store
            .bots_agent_usage_record(
                a,
                TurnUsage {
                    prompt_tokens: 80,
                    completion_tokens: 20,
                },
            )
            .unwrap();
        let status = store.bots_agent_budget_status_local(a).unwrap();
        assert!(status.over_budget()); // exactly at the cap counts as over

        // Replacing the cap overwrites, it doesn't add.
        store.bots_agent_budget_set_local(a, 1000).unwrap();
        let status = store.bots_agent_budget_status_local(a).unwrap();
        assert_eq!(status.monthly_token_limit, Some(1000));
        assert!(!status.over_budget());

        store.bots_agent_budget_clear_local(a).unwrap();
        let status = store.bots_agent_budget_status_local(a).unwrap();
        assert!(status.monthly_token_limit.is_none());
        // Usage tracking survives clearing the cap.
        assert_eq!(status.total_tokens(), 100);

        assert!(store.bots_agent_budget_set_local(a, 0).is_err());
        assert!(store.bots_agent_budget_set_local(a, -5).is_err());
    }

    #[test]
    fn set_status_and_clear_reject_an_agent_that_does_not_exist() {
        let store = LocalHubStore::in_memory().unwrap();
        let phantom = Uuid::new_v4();
        assert!(store.bots_agent_budget_set_local(phantom, 100).is_err());
        assert!(store.bots_agent_budget_status_local(phantom).is_err());
        assert!(store.bots_agent_budget_clear_local(phantom).is_err());
    }

    #[test]
    fn list_omits_untouched_agents_and_includes_capped_or_used_ones() {
        let store = LocalHubStore::in_memory().unwrap();
        let untouched = make_agent(&store);
        let capped_only = make_agent(&store);
        let used_only = make_agent(&store);
        let _ = untouched;
        store.bots_agent_budget_set_local(capped_only, 500).unwrap();
        store
            .bots_agent_usage_record(
                used_only,
                TurnUsage {
                    prompt_tokens: 10,
                    completion_tokens: 1,
                },
            )
            .unwrap();

        let listed: Vec<Uuid> = store
            .bots_agent_budget_list_local()
            .unwrap()
            .into_iter()
            .map(|s| s.agent)
            .collect();
        assert!(listed.contains(&capped_only));
        assert!(listed.contains(&used_only));
        assert!(!listed.contains(&untouched));
    }
}
