//! Trusted host staging for private coding jobs. No token or caller-supplied capability JSON.
use super::*;

pub(super) const RECEIPT: &str = "__hive_private_submission_v1";
pub(super) const WAITING: &str = "awaiting_repository_preparation";

#[derive(Clone, Serialize, Deserialize)]
pub struct PrivateCodeTaskStatus {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub reason: Option<String>,
    pub workspace: Option<String>,
    pub output: Option<String>,
    pub check_count: u32,
    pub model_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateCodeTaskRequest {
    /// Owner requests a separate, read-only review of this completed coding task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_source_task_id: Option<Uuid>,
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub target_node_id: Uuid,
    /// Owner-selected persona; omission preserves legacy anonymous tasks and receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<Uuid>,
    pub title: String,
    pub task: String,
    pub model_id: Option<String>,
    pub max_turns: u32,
    /// Owner-selected reasoning mode. Omission retains model-default behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coding_think: Option<bool>,
    /// Explicit owner opt-in; omit zero to preserve legacy submission receipts.
    #[serde(default, skip_serializing_if = "is_zero_repairs")]
    pub max_acceptance_repairs: u32,
    /// Separately authorized agent-invoked check runs; omitted zero keeps legacy receipts stable.
    #[serde(default, skip_serializing_if = "is_zero_repairs")]
    pub max_verification_runs: u32,
    pub acceptance: Vec<crate::acceptance::AcceptanceCheck>,
}

fn is_zero_repairs(value: &u32) -> bool {
    *value == 0
}

impl LocalHubStore {
    pub fn private_code_task_statuses(
        &self,
        project: Uuid,
        node: Uuid,
    ) -> Result<Vec<PrivateCodeTaskStatus>> {
        self.transaction(|tx| {
            let mut query = tx.prepare("SELECT c.data,c.status,c.reason,(SELECT substr(o.content,1,16000) FROM card_outputs o WHERE o.card_id=c.id) FROM cards c WHERE c.project_id=?1 ORDER BY c.rowid DESC").map_err(db_error)?;
            let rows = query.query_map([project.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?))).map_err(db_error)?;
            let mut result = Vec::new();
            for row in rows {
                let (raw, status, reason, output) = row.map_err(db_error)?;
                let card: ClaimedCard = decode(&raw)?;
                let Some(receipt) = card.required_capabilities.get(RECEIPT) else { continue; };
                let request: PrivateCodeTaskRequest = serde_json::from_value(receipt.clone()).map_err(|_| rejected("invalid private submission receipt"))?;
                if request.target_node_id != node { continue; }
                result.push(PrivateCodeTaskStatus { id: card.id, title: card.title, status, reason, output, check_count:request.acceptance.len() as u32, model_id:request.model_id, workspace: card.required_capabilities.get("prepared_workspace_root").and_then(Value::as_str).map(str::to_owned) });
            }
            Ok(result)
        })
    }
    /// Trusted host only. `executing_node` must come from the host's verified identity.
    /// Credentials are used only during fresh preparation and never enter stored card data.
    #[cfg(feature = "sandbox")]
    pub async fn prepare_private_code_task(
        &self,
        id: Uuid,
        executing_node: Uuid,
        data: &std::path::Path,
        token: &str,
    ) -> Result<std::path::PathBuf> {
        let card = self.transaction(|tx| preparation_card(tx, id, executing_node))?;
        let raw = encode(&card)?;
        if card
            .required_capabilities
            .get("independent_review")
            .is_some()
        {
            self.transaction(|tx| {
                let current = preparation_card(tx, id, executing_node)?;
                if encode(&current)? != raw { return Err(rejected("review task changed during preparation")); }
                validate_review_source(tx, &current)?;
                tx.execute(
                    "UPDATE cards SET status='ready',reason=NULL,data=json_set(data,'$.required_capabilities.prepared_workspace_root','/frozen-review-package') WHERE id=?1",
                    [id.to_string()],
                )
                .map_err(db_error)?;
                Ok(())
            })?;
            return Ok(std::path::PathBuf::from("/frozen-review-package"));
        }
        let spec =
            crate::coder::CodeSessionSpec::from_required_capabilities(&card.required_capabilities)
                .map_err(|_| rejected("invalid staged coding specification"))?;
        let prepared = crate::coder::workspace::prepare_authenticated(data, id, &spec, token)
            .await
            .map_err(|e| rejected(&e.to_string()))?;
        // Release the preparation lock before making the card claimable, so a fast worker
        // cannot mistake this host operation for a competing coding session. The worker
        // reacquires the managed lock and revalidates the receipt before any model turn.
        let root = prepared.root.clone();
        drop(prepared);
        // Recheck target, status and payload after network/filesystem work.
        self.transaction(|tx| {
            let mut current = preparation_card(tx, id, executing_node)?;
            if encode(&current)? != raw {
                return Err(rejected("staged coding task changed during preparation"));
            }
            current.required_capabilities["prepared_workspace_root"] = json!(root
                .to_str()
                .ok_or_else(|| rejected("non-UTF8 workspace path"))?);
            current.requires_internet = false; // All Git download work is finished before activation.
            tx.execute(
                "UPDATE cards SET data=?2,status='ready',reason=NULL WHERE id=?1",
                params![id.to_string(), encode(&current)?],
            )
            .map_err(db_error)?;
            Ok(())
        })?;
        Ok(root)
    }

    /// Explicit owner retry: validate the existing checkout without credentials or reset.
    #[cfg(feature = "sandbox")]
    pub async fn retry_private_code_task(
        &self,
        id: Uuid,
        node: Uuid,
        data: &std::path::Path,
    ) -> Result<()> {
        let card = self.transaction(|tx| private_task_card(tx, id, node, true))?;
        let raw = encode(&card)?;
        let spec =
            crate::coder::CodeSessionSpec::from_required_capabilities(&card.required_capabilities)
                .map_err(|_| rejected("invalid coding specification"))?;
        let prepared = crate::coder::workspace::prepare_authenticated(data, id, &spec, "")
            .await
            .map_err(|e| rejected(&e.to_string()))?;
        drop(prepared);
        self.transaction(|tx| {
            let current = private_task_card(tx, id, node, true)?;
            if encode(&current)? != raw { return Err(rejected("task changed during recovery")); }
            let prior: (String, Option<String>) = tx.query_row("SELECT status,reason FROM cards WHERE id=?1", [id.to_string()], |r| Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
            tx.execute("INSERT INTO activity(node_id,kind,body,payload,created) VALUES(?1,'private_task_retry','Owner requested a fresh attempt in the existing checkout',?2,?3)", params![node.to_string(), encode(&json!({"card_id":id,"prior_status":prior.0,"prior_reason":prior.1}))?, now()]).map_err(db_error)?;
            tx.execute("DELETE FROM leases WHERE card_id=?1", [id.to_string()]).map_err(db_error)?;
            tx.execute("UPDATE cards SET status='ready',reason=NULL WHERE id=?1", [id.to_string()]).map_err(db_error)?;
            Ok(())
        })
    }

    /// Owner-approved administration only; NOT exposed through paired-worker RPC.
    /// Stages a non-claimable card. A future host preparation operation must verify its
    /// workspace before publishing it to the runnable queue. Same request ID + input returns
    /// the original frozen card even after the project repository changes or is disconnected.
    pub fn stage_private_code_task(&self, request: &PrivateCodeTaskRequest) -> Result<ClaimedCard> {
        self.transaction(|tx| stage_task(tx, request))
    }
}

/// Snapshot only a verified owner's active local agent on the selected execution host.
/// The snapshot excludes avatar/credentials and does not copy chat tool grants into code tools.
pub(super) fn agent_snapshot(
    tx: &Transaction<'_>,
    request: &PrivateCodeTaskRequest,
) -> Result<Option<Value>> {
    let Some(agent) = request.agent_id else {
        return Ok(None);
    };
    if agent.is_nil() {
        return Err(rejected("choose an active local coding agent"));
    }
    let owner = verified_owner(tx, &request.target_node_id.to_string())?;
    let row: Option<(String, u32)> = tx.query_row(
        "SELECT name,role_revision FROM agent_profiles WHERE id=?1 AND owner=?2 AND runtime_kind='local' AND preferred_host=?3 AND archived=0",
        params![agent.to_string(), owner, request.target_node_id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional().map_err(db_error)?;
    let (name, role_revision) = row.ok_or_else(|| {
        rejected("agent must be active, owned by this fleet and assigned to the execution computer")
    })?;
    let bio: Option<(String, String, u32)> = tx
        .query_row(
            "SELECT bio,instructions,revision FROM bots_agent_bios WHERE agent=?1",
            [agent.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?;
    let (bio, instructions, bio_revision) = bio.unwrap_or_default();
    Ok(Some(
        json!({"id":agent, "name":name, "host":request.target_node_id,
        "role_revision":role_revision, "bio_revision":bio_revision,
        "bio":bio, "instructions":instructions}),
    ))
}

/// Reassignment/archive stops future execution; harmless profile edits do not rewrite the
/// immutable persona accepted at submission. Anonymous legacy cards are unchanged.
pub(super) fn validate_agent_assignment(tx: &Transaction<'_>, card: &ClaimedCard) -> Result<()> {
    let Some(receipt) = card.required_capabilities.get(RECEIPT) else {
        return Ok(());
    };
    let request: PrivateCodeTaskRequest = serde_json::from_value(receipt.clone())
        .map_err(|_| rejected("invalid private submission receipt"))?;
    agent_snapshot(tx, &request)?;
    validate_review_source(tx, card)?;
    super::private_correction::validate_source(tx, card)?;
    Ok(())
}

pub(super) fn stage_task(
    tx: &Transaction<'_>,
    request: &PrivateCodeTaskRequest,
) -> Result<ClaimedCard> {
    if request.request_id.is_nil()
        || request.project_id.is_nil()
        || request.target_node_id.is_nil()
        || request.max_turns == 0
        || request.max_turns > 100
        || request.max_acceptance_repairs > 3
        || request.max_verification_runs > 3
        || (request.max_verification_runs > 0 && request.acceptance.is_empty())
    {
        return Err(rejected(
            "invalid private coding request identity or turn limit",
        ));
    }
    check_text(&request.title, 1000)?;
    check_text(&request.task, 100_000)?;
    if let Some(model) = &request.model_id {
        check_text(model, 500)?;
    }
    crate::acceptance::validate(&request.acceptance).map_err(rejected)?;
    let receipt = serde_json::to_value(request).map_err(|_| rejected("invalid submission"))?;

    let existing: Option<String> = tx
        .query_row(
            "SELECT data FROM cards WHERE id=?1",
            [request.request_id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(raw) = existing {
        let card: ClaimedCard = decode(&raw)?;
        if card.required_capabilities.get(RECEIPT) != Some(&receipt)
            || card.project_id != request.project_id
        {
            return Err(rejected(
                "submission request ID conflicts with existing work",
            ));
        }
        return Ok(card);
    }
    let agent = agent_snapshot(tx, request)?;
    let review = match request.review_source_task_id {
        Some(source) => Some(review_source(tx, request, source)?),
        None => None,
    };
    let task = match &agent {
        Some(context) => format!("Owner-selected coding agent context: {}. This identity and biography do not grant additional tools, Library access or delegation. The selected model and execution computer are authoritative.\n\nProject task:\n{}", context, request.task),
        None => request.task.clone(),
    };
    let target: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM nodes n JOIN local_node_keys k ON k.node_id=n.id WHERE n.id=?1 AND k.revoked=0)", [request.target_node_id.to_string()], |r| r.get(0)).map_err(db_error)?;
    if !target {
        return Err(rejected(
            "target computer is not enrolled or has been revoked",
        ));
    }
    let mut card = ClaimedCard {
        id: request.request_id,
        project_id: request.project_id,
        key: format!("private-code-{}", request.request_id),
        title: request.title.clone(),
        modality: "code".into(),
        inputs: request.task.clone(),
        acceptance: "Run the owner's explicit acceptance checks; otherwise report unverified."
            .into(),
        deps: request
            .review_source_task_id
            .into_iter()
            .map(|id| format!("private-code-{id}"))
            .collect(),
        requires_internet: request.review_source_task_id.is_none(),
        required_capabilities: json!({
            "brain":"local", "task":task, "max_turns":request.max_turns,
            "model_id":request.model_id, "target_node_id":request.target_node_id,
            "max_acceptance_repairs":request.max_acceptance_repairs,
            "max_verification_runs":request.max_verification_runs,
            "acceptance":request.acceptance, (RECEIPT):receipt
        }),
    };
    if let Some(think) = request.coding_think {
        card.required_capabilities["coding_think"] = json!(think);
    }
    if let Some(agent) = agent {
        card.required_capabilities["__hive_private_agent_v1"] = agent;
    }
    if let Some(review) = review {
        card.required_capabilities["independent_review"] =
            serde_json::to_value(review).map_err(|_| rejected("invalid review request"))?;
        card.requires_internet = false;
    } else if let Some(agent) = request.agent_id {
        card.required_capabilities["review_capture"] =
            json!({"task_id":card.id,"agent_id":agent,"original_task":request.task});
    }
    repository::apply_project_default(tx, &mut card)?;
    if request.review_source_task_id.is_none()
        && card
            .required_capabilities
            .get("repo_url")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(rejected(
            "connect a project repository before staging a coding task",
        ));
    }
    validate_card(&card)?;
    tx.execute(
        "INSERT INTO cards(id,project_id,key,data,status,reason) VALUES(?1,?2,?3,?4,'blocked',?5)",
        params![
            card.id.to_string(),
            card.project_id.to_string(),
            card.key,
            encode(&card)?,
            WAITING
        ],
    )
    .map_err(db_error)?;
    Ok(card)
}

#[cfg(feature = "sandbox")]
fn preparation_card(tx: &Transaction<'_>, id: Uuid, node: Uuid) -> Result<ClaimedCard> {
    private_task_card(tx, id, node, false)
}

#[cfg(feature = "sandbox")]
fn private_task_card(
    tx: &Transaction<'_>,
    id: Uuid,
    node: Uuid,
    retry: bool,
) -> Result<ClaimedCard> {
    let (raw, status, reason): (String, String, Option<String>) = tx
        .query_row(
            "SELECT data,status,reason FROM cards WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db_error)?;
    if retry {
        if !((status == "blocked" && reason.as_deref() != Some(WAITING)) || status == "running") {
            return Err(rejected(
                "only interrupted or failed prepared tasks can be retried",
            ));
        }
        let unsafe_state: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM leases WHERE card_id=?1 AND expires>?2) OR EXISTS(SELECT 1 FROM card_outputs WHERE card_id=?1) OR EXISTS(SELECT 1 FROM child_links WHERE parent=?1) OR EXISTS(SELECT 1 FROM checkpoints WHERE card_id=?1)", params![id.to_string(), now()], |r| r.get(0)).map_err(db_error)?;
        if unsafe_state {
            return Err(rejected("task has an active lease, result, child work or checkpoint; automatic recovery is unavailable"));
        }
    } else if !(status == "ready" || (status == "blocked" && reason.as_deref() == Some(WAITING))) {
        return Err(rejected(
            "task is not awaiting preparation or already ready",
        ));
    }
    let card: ClaimedCard = decode(&raw)?;
    let request: PrivateCodeTaskRequest = serde_json::from_value(
        card.required_capabilities
            .get(RECEIPT)
            .cloned()
            .ok_or_else(|| rejected("not an owner-staged private task"))?,
    )
    .map_err(|_| rejected("invalid private submission receipt"))?;
    if request.request_id != id
        || request.project_id != card.project_id
        || request.target_node_id != node
        || card
            .required_capabilities
            .get("target_node_id")
            .and_then(Value::as_str)
            != Some(node.to_string().as_str())
    {
        return Err(rejected("private task belongs to another computer"));
    }
    validate_agent_assignment(tx, &card)?;
    let enrolled: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM local_node_keys WHERE node_id=?1 AND revoked=0)",
            [node.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !enrolled {
        return Err(rejected("target computer was revoked"));
    }
    if (status == "ready" || retry)
        && card
            .required_capabilities
            .get("prepared_workspace_root")
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(rejected("ready task has no preparation receipt"));
    }
    Ok(card)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_agent_tasks_freeze_persona_and_reject_wrong_owner_host_or_runtime() {
        use crate::bots::{AgentBio, AgentRuntimeKind, NewAgentProfile};
        let store = LocalHubStore::in_memory().unwrap();
        let host = store.enroll_owner("Execution host").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(host.node_id, owner).unwrap();
        store.transaction(|tx| {
            tx.execute("UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust='fixture' WHERE id=1",params![Uuid::new_v4().to_string(),owner.to_string()]).unwrap();
            tx.execute("INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",params![Uuid::new_v4().to_string(),host.node_id.to_string(),now()]).unwrap();
            Ok(())
        }).unwrap();
        let hub = store.connect(&host.raw_key).unwrap();
        let project = store.create_project("Named coder", "Fixture").unwrap();
        store
            .set_project_repository(
                project,
                Some(&repository::ProjectRepository {
                    repo_url: "https://github.com/example/fixture.git".into(),
                    repo_ref: None,
                }),
            )
            .unwrap();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Tyr".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(host.node_id),
                capability_policy_ref: "developer-v1".into(),
                provider_account_ref: None,
                memory_namespace: "fixture".into(),
            })
            .unwrap();
        store
            .bots_agent_bio_set(
                owner,
                agent.id,
                "Tyr".into(),
                AgentBio {
                    bio: "Our coder".into(),
                    instructions: "Keep changes small and check the behavior.".into(),
                    avatar: "tyr".into(),
                    revision: 0,
                },
            )
            .unwrap();
        let mut request = PrivateCodeTaskRequest {
            review_source_task_id: None,
            coding_think: None,
            max_acceptance_repairs: 1,
            max_verification_runs: 0,
            request_id: Uuid::new_v4(),
            project_id: project,
            target_node_id: host.node_id,
            agent_id: Some(agent.id),
            title: "Small task".into(),
            task: "Update readme".into(),
            model_id: Some("fixture".into()),
            max_turns: 2,
            acceptance: vec![],
        };
        request.coding_think = Some(false);
        let card = hub.private_code_task_stage(&request).unwrap();
        assert_eq!(card.required_capabilities["coding_think"], false);
        assert_eq!(card.required_capabilities[RECEIPT]["coding_think"], false);
        let mut changed_policy = request.clone();
        changed_policy.coding_think = None;
        assert!(hub.private_code_task_stage(&changed_policy).is_err());
        assert_eq!(card.required_capabilities["max_acceptance_repairs"], 1);
        let mut invalid = request.clone();
        invalid.request_id = Uuid::new_v4();
        invalid.max_acceptance_repairs = 4;
        assert!(hub.private_code_task_stage(&invalid).is_err());
        let mut verification_request = request.clone();
        verification_request.request_id = Uuid::new_v4();
        verification_request.max_verification_runs = 1;
        assert!(
            hub.private_code_task_stage(&verification_request).is_err(),
            "Missing checks must refuse opt-in"
        );
        verification_request.acceptance = vec![crate::acceptance::AcceptanceCheck {
            name: "Frozen check".into(),
            command: "/usr/bin/true".into(),
            args: vec![],
            cwd: None,
            expect_exit: 0,
            required: true,
        }];
        let verified_card = hub.private_code_task_stage(&verification_request).unwrap();
        assert_eq!(
            verified_card.required_capabilities["max_verification_runs"],
            1
        );
        assert_eq!(
            verified_card.required_capabilities[RECEIPT]["max_verification_runs"],
            1
        );
        verification_request.max_verification_runs = 2;
        assert!(
            hub.private_code_task_stage(&verification_request).is_err(),
            "Same request cannot broaden execution quota"
        );
        verification_request.request_id = Uuid::new_v4();
        verification_request.max_verification_runs = 4;
        assert!(hub.private_code_task_stage(&verification_request).is_err());
        let mut legacy = request.clone();
        legacy.max_acceptance_repairs = 0;
        legacy.coding_think = None;
        let wire = serde_json::to_value(&legacy).unwrap();
        assert!(wire.get("max_acceptance_repairs").is_none());
        assert!(wire.get("max_verification_runs").is_none());
        assert!(wire.get("coding_think").is_none());
        let mut malformed = wire.clone();
        malformed["coding_think"] = json!("false");
        assert!(serde_json::from_value::<PrivateCodeTaskRequest>(malformed).is_err());
        assert_eq!(
            serde_json::from_value::<PrivateCodeTaskRequest>(wire.clone())
                .unwrap()
                .coding_think,
            None
        );
        assert_eq!(
            serde_json::from_value::<PrivateCodeTaskRequest>(wire)
                .unwrap()
                .max_acceptance_repairs,
            0
        );

        assert!(card.required_capabilities["task"]
            .as_str()
            .unwrap()
            .contains("Keep changes small"));
        assert_eq!(
            card.required_capabilities["__hive_private_agent_v1"]["name"],
            "Tyr"
        );
        assert!(card.required_capabilities["__hive_private_agent_v1"]
            .get("avatar")
            .is_none());
        assert!(card.required_capabilities.get("coordinator").is_none());
        assert!(card.required_capabilities.get("vault_name").is_none());
        assert_eq!(
            hub.private_coding_tasks(project).unwrap()[0]
                .agent_name
                .as_deref(),
            Some("Tyr")
        );
        // A later biography edit cannot reinterpret a staged task or duplicate the request.
        store
            .bots_agent_bio_set(
                owner,
                agent.id,
                "Renamed".into(),
                AgentBio {
                    bio: "New bio".into(),
                    instructions: "Different instructions".into(),
                    avatar: "".into(),
                    revision: 1,
                },
            )
            .unwrap();
        assert_eq!(
            hub.private_code_task_stage(&request)
                .unwrap()
                .required_capabilities,
            card.required_capabilities
        );
        let anonymous = {
            let mut r = request.clone();
            r.agent_id = None;
            r
        };
        assert!(serde_json::to_value(&anonymous)
            .unwrap()
            .get("agent_id")
            .is_none());
        let decoded: PrivateCodeTaskRequest =
            serde_json::from_value(serde_json::to_value(&anonymous).unwrap()).unwrap();
        assert!(decoded.agent_id.is_none());
        request.request_id = Uuid::new_v4();
        for (column, bad, good) in [
            ("owner", Uuid::new_v4().to_string(), owner.to_string()),
            (
                "preferred_host",
                Uuid::new_v4().to_string(),
                host.node_id.to_string(),
            ),
            ("runtime_kind", "anthropic_byok".into(), "local".into()),
            ("archived", "1".into(), "0".into()),
        ] {
            store
                .transaction(|tx| {
                    tx.execute(
                        &format!("UPDATE agent_profiles SET {column}=?1 WHERE id=?2"),
                        params![bad, agent.id.to_string()],
                    )
                    .unwrap();
                    assert!(validate_agent_assignment(tx, &card).is_err());
                    assert!(super::super::private_run::allows_claim(
                        tx,
                        &host.node_id.to_string(),
                        &card,
                        None,
                        Uuid::new_v4()
                    )
                    .is_err());
                    Ok(())
                })
                .unwrap();
            assert!(hub.private_code_task_stage(&request).is_err());
            store
                .transaction(|tx| {
                    tx.execute(
                        &format!("UPDATE agent_profiles SET {column}=?1 WHERE id=?2"),
                        params![good, agent.id.to_string()],
                    )
                    .unwrap();
                    Ok(())
                })
                .unwrap();
        }
        assert!(store
            .transaction(|tx| validate_agent_assignment(tx, &card))
            .is_ok());
        let mut conflict = request.clone();
        conflict.request_id = card.id;
        conflict.agent_id = None;
        assert!(hub.private_code_task_stage(&conflict).is_err());
    }

    #[test]
    fn status_reports_frozen_checks_and_rejects_invalid_submission() {
        let store = LocalHubStore::in_memory().unwrap();
        let node = store.enroll_owner("test").unwrap().node_id;
        let project = store.create_project("test", "checks").unwrap();
        store
            .set_project_repository(
                project,
                Some(&repository::ProjectRepository {
                    repo_url: "https://github.com/example/test.git".into(),
                    repo_ref: None,
                }),
            )
            .unwrap();
        let mut request = PrivateCodeTaskRequest {
            review_source_task_id: None,
            coding_think: None,
            max_acceptance_repairs: 0,
            max_verification_runs: 0,
            request_id: Uuid::new_v4(),
            project_id: project,
            target_node_id: node,
            agent_id: None,
            title: "Test checks".into(),
            task: "Test".into(),
            model_id: None,
            max_turns: 2,
            acceptance: vec![crate::acceptance::AcceptanceCheck {
                name: "Unit tests".into(),
                command: "npm".into(),
                args: vec!["test".into()],
                cwd: None,
                expect_exit: 0,
                required: true,
            }],
        };
        let card = store.stage_private_code_task(&request).unwrap();
        assert_eq!(
            card.required_capabilities["acceptance"][0]["args"][0],
            "test"
        );
        assert_eq!(
            store.private_code_task_statuses(project, node).unwrap()[0].check_count,
            1
        );
        request.acceptance[0].args = vec!["different".into()];
        assert!(store.stage_private_code_task(&request).is_err());
        request.request_id = Uuid::new_v4();
        request.acceptance[0].command = String::new();
        assert!(store.stage_private_code_task(&request).is_err());
        assert_eq!(
            store
                .private_code_task_statuses(project, node)
                .unwrap()
                .len(),
            1
        );
    }
}

/// Enrollment is identity, not liveness or coding readiness. Capability discovery is separate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrivateExecutionHost {
    pub node_id: Uuid,
    pub name: String,
}

pub(super) fn verified_owner(tx: &Transaction<'_>, node: &str) -> Result<String> {
    tx.query_row(
        "SELECT a.owner_id FROM private_fleet_authority a JOIN nodes n ON n.owner_member_id=a.owner_id WHERE a.id=1 AND a.fleet_id IS NOT NULL AND a.trust IS NOT NULL AND n.id=?1 AND EXISTS(SELECT 1 FROM private_fleet_enrollments e WHERE e.node_id=n.id)",
        [node], |r| r.get(0),
    ).optional().map_err(db_error)?.ok_or_else(|| rejected("verified Private Fleet enrollment is required"))
}
pub(super) fn verify_target(tx: &Transaction<'_>, owner: &str, target: Uuid) -> Result<()> {
    let valid: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes n WHERE n.id=?1 AND n.owner_member_id=?2 AND EXISTS(SELECT 1 FROM private_fleet_enrollments e WHERE e.node_id=n.id) AND EXISTS(SELECT 1 FROM local_node_keys k WHERE k.node_id=n.id AND k.revoked=0))",
        params![target.to_string(), owner], |r| r.get(0),
    ).map_err(db_error)?;
    if !valid {
        return Err(rejected(
            "execution computer is not enrolled in this Private Fleet or has been revoked",
        ));
    }
    Ok(())
}
impl LocalHub {
    pub fn private_execution_hosts(&self) -> Result<Vec<PrivateExecutionHost>> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let mut query = tx.prepare("SELECT n.id,n.name FROM nodes n WHERE n.owner_member_id=?1 AND EXISTS(SELECT 1 FROM private_fleet_enrollments e WHERE e.node_id=n.id) AND EXISTS(SELECT 1 FROM local_node_keys k WHERE k.node_id=n.id AND k.revoked=0) ORDER BY n.name,n.id").map_err(db_error)?;
            let rows = query.query_map([owner], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(db_error)?;
            rows.map(|row| {
                let (id, name) = row.map_err(db_error)?;
                Ok(PrivateExecutionHost { node_id: Uuid::parse_str(&id).map_err(|_| rejected("invalid execution host identity"))?, name })
            }).collect()
        })
    }

    /// Authenticated owner submission. Authorization and immutable staging share one transaction.
    /// Staging alone never prepares files or makes work runnable.
    pub fn private_code_task_stage(&self, request: &PrivateCodeTaskRequest) -> Result<ClaimedCard> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            verify_target(tx, &owner, request.target_node_id)?;
            stage_task(tx, request)
        })
    }
}

#[cfg(feature = "sandbox")]
fn review_source(
    tx: &Transaction<'_>,
    request: &PrivateCodeTaskRequest,
    source: Uuid,
) -> Result<crate::coder::checker::ReviewRequest> {
    use crate::coder::checker::{ReviewPackage, ReviewRequest};
    if !request.acceptance.is_empty()
        || request.max_acceptance_repairs != 0
        || request.max_verification_runs != 0
        || source == request.request_id
    {
        return Err(rejected(
            "checker cannot execute commands, repair or review itself",
        ));
    }
    let checker = request
        .agent_id
        .ok_or_else(|| rejected("select a distinct checker agent"))?;
    let (raw, state, output): (String, String, Option<String>) = tx.query_row(
        "SELECT c.data,c.status,o.content FROM cards c LEFT JOIN card_outputs o ON o.card_id=c.id WHERE c.id=?1 AND c.project_id=?2",
        params![source.to_string(), request.project_id.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_error)?;
    if state != "review" && state != "done" {
        return Err(rejected("source coder has not finished successfully"));
    }
    let card: ClaimedCard = decode(&raw)?;
    let submission: PrivateCodeTaskRequest = serde_json::from_value(
        card.required_capabilities
            .get(RECEIPT)
            .cloned()
            .ok_or_else(|| rejected("source must be a private owner-staged task"))?,
    )
    .map_err(|_| rejected("invalid source task"))?;
    let owner = verified_owner(tx, &request.target_node_id.to_string())?;
    verify_target(tx, &owner, submission.target_node_id)?;
    if submission.review_source_task_id.is_some() {
        return Err(rejected("source must be coding work, not a checker"));
    }
    let package =
        ReviewPackage::from_output(&output.ok_or_else(|| rejected("source has no result"))?)
            .map_err(|e| rejected(&e.to_string()))?;
    if package.snapshot.identity.task_id != source
        || Some(package.snapshot.identity.agent_id) != submission.agent_id
        || package.snapshot.identity.original_task != submission.task
    {
        return Err(rejected("review package does not match source submission"));
    }
    let review = ReviewRequest {
        checker_agent_id: checker,
        package,
    };
    review.validate().map_err(|e| rejected(&e.to_string()))?;
    Ok(review)
}
#[cfg(not(feature = "sandbox"))]
fn review_source(
    _tx: &Transaction<'_>,
    _request: &PrivateCodeTaskRequest,
    _source: Uuid,
) -> Result<Value> {
    Err(rejected("independent checker requires the coding engine"))
}
pub(super) fn validate_review_source(tx: &Transaction<'_>, card: &ClaimedCard) -> Result<()> {
    let Some(stored) = card.required_capabilities.get("independent_review") else {
        return Ok(());
    };
    let request: PrivateCodeTaskRequest = serde_json::from_value(
        card.required_capabilities
            .get(RECEIPT)
            .cloned()
            .ok_or_else(|| rejected("checker missing owner submission"))?,
    )
    .map_err(|_| rejected("invalid checker submission"))?;
    let source = request
        .review_source_task_id
        .ok_or_else(|| rejected("checker missing source task"))?;
    let current = serde_json::to_value(review_source(tx, &request, source)?)
        .map_err(|_| rejected("invalid review source"))?;
    if &current != stored {
        return Err(rejected(
            "review source changed; stage a fresh checker request",
        ));
    }
    Ok(())
}

#[cfg(all(test, feature = "sandbox"))]
mod checker_tests {
    use super::*;
    use crate::bots::{AgentRuntimeKind, NewAgentProfile};
    use crate::coder::checker::*;
    use sha2::{Digest, Sha256};
    #[tokio::test]
    async fn checker_staging_freezes_owner_source_and_preserves_coder_without_checkout() {
        let store = LocalHubStore::in_memory().unwrap();
        let host = store.enroll_owner("worker").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(host.node_id, owner).unwrap();
        store.transaction(|tx| {tx.execute("UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust='fixture' WHERE id=1",params![Uuid::new_v4().to_string(),owner.to_string()]).unwrap();tx.execute("INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",params![Uuid::new_v4().to_string(),host.node_id.to_string(),now()]).unwrap();Ok(())}).unwrap();
        let hub = store.connect(&host.raw_key).unwrap();
        let project = store.create_project("checker", "fixture").unwrap();
        store
            .set_project_repository(
                project,
                Some(&repository::ProjectRepository {
                    repo_url: "https://github.com/example/fixture.git".into(),
                    repo_ref: None,
                }),
            )
            .unwrap();
        let agent = |name: &str| {
            store
                .bots_agents_create(NewAgentProfile {
                    owner,
                    name: name.into(),
                    runtime_kind: AgentRuntimeKind::Local,
                    preferred_host: Some(host.node_id),
                    capability_policy_ref: "developer-v1".into(),
                    provider_account_ref: None,
                    memory_namespace: "fixture".into(),
                })
                .unwrap()
        };
        let coder = agent("coder");
        let checker = agent("checker");
        let mut request = PrivateCodeTaskRequest {
            request_id: Uuid::new_v4(),
            project_id: project,
            target_node_id: host.node_id,
            agent_id: Some(coder.id),
            title: "source".into(),
            task: "Add two numbers".into(),
            model_id: Some("fixture".into()),
            max_turns: 1,
            coding_think: Some(false),
            max_acceptance_repairs: 0,
            max_verification_runs: 0,
            acceptance: vec![],
            review_source_task_id: None,
        };
        let source = hub.private_code_task_stage(&request).unwrap();
        let snapshot = ReviewSnapshot {
            identity: CaptureIdentity {
                task_id: source.id,
                agent_id: coder.id,
                original_task: request.task.clone(),
            },
            base_commit: "a".repeat(40),
            files: vec![ReviewFile {
                path: "add.py".into(),
                before: None,
                after: Some("return a - b\n".into()),
            }],
            coder_report: "Done".into(),
            acceptance: crate::coder::AcceptanceOutcome::Unverified,
            model_id: Some("fixture".into()),
        };
        let package = ReviewPackage {
            digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&snapshot).unwrap())
            ),
            snapshot,
        };
        let output = format!(
            "Done\n{PACKAGE_MARKER}{}",
            serde_json::to_string(&package).unwrap()
        );
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE cards SET status='review' WHERE id=?1",
                    [source.id.to_string()],
                )
                .unwrap();
                tx.execute(
                    "INSERT INTO card_outputs(card_id,node_id,session,content,usage) VALUES(?1,'fixture','fixture',?2,'{}')",
                    params![source.id.to_string(), output],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        request.request_id = Uuid::new_v4();
        request.review_source_task_id = Some(source.id);
        assert!(hub.private_code_task_stage(&request).is_err()); // Same persona cannot self-review.
        request.agent_id = Some(checker.id);
        request.task = "Independent check".into();
        let review = hub.private_code_task_stage(&request).unwrap();
        assert_eq!(
            review.required_capabilities["independent_review"]["package"]["digest"],
            package.digest
        );
        assert_eq!(hub.private_code_task_stage(&request).unwrap().id, review.id);
        let mut conflict = request.clone();
        conflict.coding_think = None;
        assert!(hub.private_code_task_stage(&conflict).is_err());
        let mut command = request.clone();
        command.request_id = Uuid::new_v4();
        command.max_acceptance_repairs = 1;
        assert!(hub.private_code_task_stage(&command).is_err());
        let data = std::env::temp_dir().join(format!("den-checker-no-checkout-{}", Uuid::new_v4()));
        store
            .prepare_private_code_task(review.id, host.node_id, &data, "")
            .await
            .unwrap();
        assert!(!data.exists());
        assert_eq!(
            hub.private_coding_tasks(project)
                .unwrap()
                .iter()
                .find(|c| c.task_id == source.id)
                .unwrap()
                .status,
            "review"
        );
        // A completed independent verdict creates a separate, bounded branch.
        let receipt = ReviewReceipt {
            checker_task_id: review.id,
            checker_agent_id: checker.id,
            source_task_id: source.id,
            source_agent_id: coder.id,
            package_digest: package.digest.clone(),
            base_commit: package.snapshot.base_commit.clone(),
            model_id: Some("fixture".into()),
            review: ReviewVerdict {
                verdict: Verdict::ChangesRequired,
                summary: "Fix addition".into(),
                findings: vec![Finding {
                    path: "add.py".into(),
                    line: 1,
                    message: "Subtracts instead".into(),
                    evidence: "return a - b".into(),
                }],
            },
            scope: "frozen_snapshot_only".into(),
            test_execution: "prior_coder_host_receipt_only".into(),
        };
        let verdict_output = format!(
            "Independent checker verdict: {}",
            serde_json::to_string(&receipt).unwrap()
        );
        let correction_request = super::super::private_correction::PrivateCorrectionRequest {
            request_id: Uuid::new_v4(),
            project_id: project,
            review_task_id: review.id,
        };
        assert!(hub
            .private_code_correction_stage(&correction_request)
            .is_err()); // Still unfinished.
        store.transaction(|tx| {
            tx.execute("UPDATE cards SET status='review' WHERE id=?1", [review.id.to_string()]).unwrap();
            tx.execute("INSERT INTO card_outputs(card_id,node_id,session,content,usage) VALUES(?1,'fixture','fixture',?2,'{}')", params![review.id.to_string(), verdict_output]).unwrap();
            Ok(())
        }).unwrap();
        // A changed profile cannot silently replace the submitted persona.
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE agent_profiles SET name='new profile name' WHERE id=?1",
                    [coder.id.to_string()],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        let correction = hub
            .private_code_correction_stage(&correction_request)
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server_store = store.clone();
        let server = tokio::spawn(async move {
            super::super::transport::serve(server_store, listener, async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        let remote = RemoteLocalHub::new(&url, host.raw_key.clone()).unwrap();
        assert_eq!(
            remote
                .private_code_correction_stage(&correction_request)
                .await
                .unwrap()
                .id,
            correction.id
        );
        let invalid = RemoteLocalHub::new(&url, "invalid-key".into()).unwrap();
        assert!(invalid
            .private_code_correction_stage(&correction_request)
            .await
            .is_err());
        stop.send(()).unwrap();
        server.await.unwrap();
        assert_ne!(correction.id, source.id);
        assert_eq!(
            correction.required_capabilities["task"],
            source.required_capabilities["task"]
        );
        assert_eq!(
            correction.required_capabilities["__hive_private_agent_v1"],
            source.required_capabilities["__hive_private_agent_v1"]
        );
        assert_eq!(
            correction.required_capabilities["repo_ref"],
            package.snapshot.base_commit
        );
        assert_eq!(
            correction.required_capabilities["model_id"],
            source.required_capabilities["model_id"]
        );
        assert_eq!(
            correction.required_capabilities["coding_think"],
            source.required_capabilities["coding_think"]
        );
        assert_eq!(
            correction.required_capabilities["acceptance"],
            source.required_capabilities["acceptance"]
        );
        assert_eq!(
            hub.private_code_correction_stage(&correction_request)
                .unwrap()
                .id,
            correction.id
        );
        let context: CorrectionContext =
            serde_json::from_value(correction.required_capabilities["checker_correction"].clone())
                .unwrap();
        context.validate().unwrap();
        assert_eq!(context.round, 1);
        assert!(context.prompt().contains("UNTRUSTED EVIDENCE"));
        let mut invalid = context.clone();
        invalid.round = 4;
        assert!(invalid.validate().is_err());
        invalid = context.clone();
        invalid.receipt.review.verdict = Verdict::Inconclusive;
        assert!(invalid.validate().is_err());
        invalid = context.clone();
        invalid.receipt.package_digest = "0".repeat(64);
        assert!(invalid.validate().is_err());
        invalid = context.clone();
        invalid.receipt.source_agent_id = checker.id;
        assert!(invalid.validate().is_err());
        assert!(
            ReviewReceipt::from_output(&format!("{verdict_output}\n{verdict_output}")).is_err()
        );
        store
            .transaction(|tx| {
                validate_agent_assignment(tx, &correction)?;
                tx.execute(
                    "UPDATE card_outputs SET content='changed' WHERE card_id=?1",
                    [review.id.to_string()],
                )
                .unwrap();
                assert!(validate_agent_assignment(tx, &correction).is_err());
                tx.execute(
                    "UPDATE card_outputs SET content=?2 WHERE card_id=?1",
                    params![review.id.to_string(), verdict_output],
                )
                .unwrap();
                validate_agent_assignment(tx, &correction)?;
                Ok(())
            })
            .unwrap();
        let overview = hub.private_coding_tasks(project).unwrap();
        let source_view = overview.iter().find(|c| c.task_id == source.id).unwrap();
        assert!(source_view.review_available);
        assert!(!source_view
            .output
            .as_ref()
            .unwrap()
            .contains(PACKAGE_MARKER));
        store
            .transaction(|tx| {
                validate_agent_assignment(tx, &review)?;
                tx.execute(
                    "UPDATE card_outputs SET content='changed' WHERE card_id=?1",
                    [source.id.to_string()],
                )
                .unwrap();
                assert!(validate_agent_assignment(tx, &review).is_err());
                assert!(validate_agent_assignment(tx, &correction).is_err());
                Ok(())
            })
            .unwrap();
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE card_outputs SET content=?2 WHERE card_id=?1",
                    params![source.id.to_string(), output],
                )
                .unwrap();
                Ok(())
            })
            .unwrap();
        hub.bots_agents_archive(checker.id).unwrap();
        store
            .transaction(|tx| {
                assert!(validate_agent_assignment(tx, &review).is_err());
                assert!(validate_agent_assignment(tx, &correction).is_err());
                Ok(())
            })
            .unwrap();
    }
}
