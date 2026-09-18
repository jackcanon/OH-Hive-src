//! Explicit, durable one-attempt execution authorization for remotely prepared coding tasks.
use super::private_code_tasks::{verified_owner, verify_target};
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrivateRunStatus {
    pub operation_id: Uuid,
    pub task_id: Uuid,
    pub target_node_id: Uuid,
    pub state: String,
    pub task_status: String,
    pub reason: Option<String>,
    pub lease_active: bool,
    pub stop_requested: bool,
}
fn status(tx: &Transaction<'_>, id: Uuid) -> Result<PrivateRunStatus> {
    let (task,target,state,task_status,reason,lease_active): (String,String,String,String,Option<String>,bool) = tx.query_row(
        "SELECT r.card_id,r.target_node_id,r.state,c.status,c.reason,EXISTS(SELECT 1 FROM leases l WHERE l.card_id=r.card_id AND l.session=r.session AND l.node_id=r.target_node_id AND l.expires>?2) FROM private_runs r JOIN cards c ON c.id=r.card_id WHERE r.id=?1",
        params![id.to_string(),now()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)),
    ).map_err(db_error)?;
    let stop_requested: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM private_run_stops WHERE operation_id=?1)",
            [id.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    let superseded: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM private_run_retries WHERE previous_id=?1)",
            [id.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    let state = if superseded {
        "superseded".to_owned()
    } else if stop_requested && !matches!(task_status.as_str(), "review" | "done") {
        if lease_active {
            "stopping".to_owned()
        } else {
            "stopped".to_owned()
        }
    } else if state == "running" {
        match task_status.as_str() {
            "review" | "done" => "finished".to_owned(),
            "blocked" => "blocked".to_owned(),
            _ if !lease_active => "interrupted".to_owned(),
            _ => state,
        }
    } else {
        state
    };
    Ok(PrivateRunStatus {
        operation_id: id,
        task_id: Uuid::parse_str(&task).map_err(|_| rejected("invalid task"))?,
        target_node_id: Uuid::parse_str(&target).map_err(|_| rejected("invalid target"))?,
        state,
        task_status,
        reason,
        lease_active,
        stop_requested,
    })
}
impl LocalHub {
    pub fn private_run_request(&self, operation: Uuid, task: Uuid) -> Result<PrivateRunStatus> {
        if operation.is_nil() {
            return Err(rejected("invalid run identity"));
        }
        self.with_node(|tx,node| {
            let owner = verified_owner(tx,node)?;
            let (target,prepared): (String,String) = tx.query_row("SELECT target_node_id,state FROM private_preparations WHERE card_id=?1",[task.to_string()],|r| Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
            verify_target(tx,&owner,Uuid::parse_str(&target).map_err(|_| rejected("invalid target"))?)?;
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM private_runs WHERE id=?1)",[operation.to_string()],|r|r.get(0)).map_err(db_error)?;
            if exists {
                let receipt = status(tx,operation)?;
                if receipt.task_id != task || receipt.target_node_id.to_string() != target { return Err(rejected("run request ID conflicts with existing work")); }
                return Ok(receipt);
            }
            let ready: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cards WHERE id=?1 AND status='blocked' AND reason='awaiting_private_run') AND NOT EXISTS(SELECT 1 FROM leases WHERE card_id=?1) AND NOT EXISTS(SELECT 1 FROM private_runs WHERE card_id=?1)",[task.to_string()],|r|r.get(0)).map_err(db_error)?;
            if prepared != "prepared" || !ready { return Err(rejected("task is not prepared for a new run")); }
            tx.execute("INSERT INTO private_runs(id,card_id,target_node_id,state,created) VALUES(?1,?2,?3,'queued',?4)",params![operation.to_string(),task.to_string(),target,now()]).map_err(db_error)?;
            tx.execute("UPDATE cards SET status='ready',reason=NULL WHERE id=?1",[task.to_string()]).map_err(db_error)?;
            status(tx,operation)
        })
    }
    pub fn private_run_status(&self, operation: Uuid) -> Result<PrivateRunStatus> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let receipt = status(tx, operation)?;
            verify_target(tx, &owner, receipt.target_node_id)?;
            Ok(receipt)
        })
    }
    pub fn private_run_stop(&self, operation: Uuid) -> Result<PrivateRunStatus> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let receipt = status(tx, operation)?;
            verify_target(tx, &owner, receipt.target_node_id)?;
            if matches!(receipt.state.as_str(), "finished" | "superseded") { return Ok(receipt); }
            tx.execute("INSERT OR IGNORE INTO private_run_stops VALUES(?1,?2,?3)", params![operation.to_string(),node,now()]).map_err(db_error)?;
            tx.execute("UPDATE cards SET status='blocked',reason='private_run_stopped' WHERE id=?1 AND status='ready'",[receipt.task_id.to_string()]).map_err(db_error)?;
            status(tx, operation)
        })
    }
    /// Read-only preflight payload, restricted to the assigned, enrolled execution computer.
    pub fn private_run_work(&self, operation: Uuid) -> Result<ClaimedCard> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let receipt = status(tx, operation)?;
            verify_target(tx, &owner, receipt.target_node_id)?;
            if receipt.target_node_id.to_string() != node
                || receipt.state != "queued"
                || receipt.stop_requested
            {
                return Err(rejected("run is not queued for this computer"));
            }
            let raw: String = tx
                .query_row(
                    "SELECT data FROM cards WHERE id=?1",
                    [receipt.task_id.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            decode(&raw)
        })
    }
    /// Explicit new attempt. Keeps task/workspace/checks, retires the old authorization.
    pub fn private_run_retry(&self, previous: Uuid, next: Uuid) -> Result<PrivateRunStatus> {
        if next.is_nil() || next == previous {
            return Err(rejected("retry needs a new operation identity"));
        }
        self.with_node(|tx,node| {
            let owner=verified_owner(tx,node)?;
            let prior=status(tx,previous)?;
            verify_target(tx,&owner,prior.target_node_id)?;
            let existing: Option<String>=tx.query_row("SELECT next_id FROM private_run_retries WHERE previous_id=?1",[previous.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            if let Some(existing)=existing {
                if existing != next.to_string() { return Err(rejected("this attempt already has a different retry")); }
                return status(tx,next);
            }
            if prior.lease_active || !matches!(prior.state.as_str(), "stopped" | "blocked" | "interrupted") { return Err(rejected("stop or finish the current attempt before retrying")); }
            let unsafe_state: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM leases WHERE card_id=?1 AND expires>?2) OR EXISTS(SELECT 1 FROM card_outputs WHERE card_id=?1) OR EXISTS(SELECT 1 FROM child_links WHERE parent=?1) OR EXISTS(SELECT 1 FROM checkpoints WHERE card_id=?1)",params![prior.task_id.to_string(),now()],|r|r.get(0)).map_err(db_error)?;
            if unsafe_state { return Err(rejected("task has output, child work or checkpoints requiring review")); }
            tx.execute("INSERT INTO private_runs VALUES(?1,?2,?3,'queued',NULL,?4)",params![next.to_string(),prior.task_id.to_string(),prior.target_node_id.to_string(),now()]).map_err(db_error)?;
            tx.execute("INSERT INTO private_run_retries VALUES(?1,?2,?3,?4)",params![previous.to_string(),next.to_string(),node,now()]).map_err(db_error)?;
            tx.execute("DELETE FROM leases WHERE card_id=?1 AND expires<=?2",params![prior.task_id.to_string(),now()]).map_err(db_error)?;
            tx.execute("UPDATE cards SET status='blocked',reason='awaiting_retry_validation' WHERE id=?1",[prior.task_id.to_string()]).map_err(db_error)?;
            status(tx,next)
        })
    }
    /// Target attests successful local checkout preflight before retry can be claimed.
    pub fn private_run_ready(&self, operation: Uuid) -> Result<PrivateRunStatus> {
        self.with_node(|tx,node| {
            let owner=verified_owner(tx,node)?;
            let receipt=status(tx,operation)?;
            verify_target(tx,&owner,receipt.target_node_id)?;
            if receipt.target_node_id.to_string()!=node || receipt.state!="queued" || receipt.stop_requested { return Err(rejected("run is not queued for this computer")); }
            tx.execute("UPDATE cards SET status='ready',reason=NULL WHERE id=?1 AND status='blocked' AND reason='awaiting_retry_validation'",[receipt.task_id.to_string()]).map_err(db_error)?;
            status(tx,operation)
        })
    }
    pub async fn private_run_claim(&self, operation: Uuid) -> Result<Claim> {
        let receipt = self.private_run_status(operation)?;
        let mut scoped = self.clone();
        scoped.claim_scope = Some(receipt.task_id);
        scoped.run_scope = Some(operation);
        scoped.claim_card().await
    }
}
/// Evaluated inside the same transaction as claim/lease insertion. Generic workers cannot
/// drain explicit private runs, and expired leases cannot silently authorize another attempt.
pub(super) fn allows_claim(
    tx: &Transaction<'_>,
    node: &str,
    card: &ClaimedCard,
    operation: Option<Uuid>,
    session: Uuid,
) -> Result<bool> {
    let remote: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM private_preparations WHERE card_id=?1)",
            [card.id.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !remote {
        return Ok(operation.is_none());
    }
    let Some(operation) = operation else {
        return Ok(false);
    };
    let owner = verified_owner(tx, node)?;
    let target = Uuid::parse_str(node).map_err(|_| rejected("invalid node"))?;
    verify_target(tx, &owner, target)?;
    tx.query_row("SELECT EXISTS(SELECT 1 FROM private_runs WHERE id=?1 AND card_id=?2 AND target_node_id=?3 AND state='queued' AND NOT EXISTS(SELECT 1 FROM private_run_stops WHERE operation_id=?1) AND NOT EXISTS(SELECT 1 FROM private_run_retries WHERE previous_id=?1) AND NOT EXISTS(SELECT 1 FROM private_runs old WHERE old.card_id=?2 AND old.id!=?1 AND old.session=?4))",params![operation.to_string(),card.id.to_string(),node,session.to_string()],|r|r.get(0)).map_err(db_error)
}

impl RemoteLocalHub {
    /// One explicit run on this execution host. No cloud adapter or fallback is constructed.
    /// The caller supplies this host's configured model server and local tool opt-in.
    #[cfg(all(feature = "sandbox", feature = "llama-cpp"))]
    pub async fn execute_private_run(
        &self,
        operation: Uuid,
        backend: &crate::backend::llama_cpp::LlamaCppBackend,
        coding_enabled: bool,
        data: &Path,
        mut local_stop: tokio::sync::watch::Receiver<bool>,
    ) -> Result<PrivateRunStatus> {
        use crate::backend::Backend;
        if !coding_enabled {
            return Err(rejected(
                "enable coding tools on the execution computer first",
            ));
        }
        if *local_stop.borrow() {
            return Err(rejected("execution was stopped before starting"));
        }
        let card = self.private_run_work(operation).await?;
        let model = card
            .required_capabilities
            .get("model_id")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| rejected("select an explicit model for remote execution"))?
            .to_owned();
        let mut caps = backend
            .capabilities()
            .await
            .map_err(|_| rejected("cannot reach the execution computer's model server"))?;
        if !caps.models.iter().any(|m| m.id == model) {
            return Err(rejected(
                "the selected model is not installed on the execution computer",
            ));
        }
        if backend
            .model_tool_support(&model)
            .await
            .map_err(|_| rejected("cannot check model tool support"))?
            == Some(false)
        {
            return Err(rejected("the selected model does not support coding tools"));
        }
        caps.tools_level = ToolsLevel::SandboxedTools;
        if !caps.modalities.contains(&Modality::Code) {
            caps.modalities.push(Modality::Code);
        }
        let spec =
            crate::coder::CodeSessionSpec::from_required_capabilities(&card.required_capabilities)
                .map_err(|_| rejected("invalid private coding specification"))?;
        if spec.prepared_workspace_root.is_none() {
            return Err(rejected("prepare the checkout on this computer first"));
        }
        // Validates the target's existing receipt/root/branch without downloading anything.
        let prepared = crate::coder::workspace::prepare_authenticated(data, card.id, &spec, "")
            .await
            .map_err(|_| {
                rejected("the execution computer's checkout needs inspection before running")
            })?;
        drop(prepared); // Worker reacquires/revalidates the same managed lock for execution.
        if *local_stop.borrow() {
            return Err(rejected("execution was stopped before starting"));
        }
        self.private_run_ready(operation).await?;
        let scoped = self.clone().for_private_run(operation);
        scoped.check_in(&caps, None).await?;
        let (stop, rx) = tokio::sync::watch::channel(false);
        let worker = crate::worker::Worker {
            #[cfg(test)]
            capacity_path: data.join("private-test-capacity"),
            hub: &scoped,
            backend,
            caps: &caps,
            default_model: Some(model),
            stop: rx,
            events: None,
            data_dir: data.to_owned(),
            sandbox: None,
        };
        let tick = worker.tick_with_heartbeat();
        tokio::pin!(tick);
        let mut poll = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tokio::select! {
                result = &mut tick => {
                    result.map_err(|_| rejected("private execution stopped or failed; inspect the task status"))?;
                    return self.private_run_status(operation).await;
                }
                changed = local_stop.changed() => {
                    if changed.is_ok() && !*local_stop.borrow() { continue; }
                    let _ = stop.send(true);
                    let _ = tick.await;
                    return self.private_run_status(operation).await;
                }
                _ = poll.tick() => {
                    match self.private_run_status(operation).await {
                        Ok(state) if state.stop_requested || state.state == "superseded" => {
                            let _ = stop.send(true);
                            let _ = tick.await;
                            return self.private_run_status(operation).await;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            // Loss of authority contact stops work locally; never select another hub.
                            let _ = stop.send(true);
                            let _ = tick.await;
                            return Err(error);
                        }
                    }
                }
            }
        }
    }
}
