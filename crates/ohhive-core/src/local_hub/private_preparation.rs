//! Durable target-pulled checkout preparation. Completion does not authorize execution.
use super::private_code_tasks::{
    verified_owner, verify_target, PrivateCodeTaskRequest, RECEIPT, WAITING,
};
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreparationStatus {
    pub operation_id: Uuid,
    pub task_id: Uuid,
    pub target_node_id: Uuid,
    pub state: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PreparationWork {
    pub operation_id: Uuid,
    pub card: ClaimedCard,
}
fn status(tx: &Transaction<'_>, id: Uuid) -> Result<PreparationStatus> {
    let (task, target, state): (String, String, String) = tx
        .query_row(
            "SELECT card_id,target_node_id,state FROM private_preparations WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db_error)?;
    Ok(PreparationStatus {
        operation_id: id,
        task_id: Uuid::parse_str(&task).map_err(|_| rejected("invalid task identity"))?,
        target_node_id: Uuid::parse_str(&target)
            .map_err(|_| rejected("invalid target identity"))?,
        state,
    })
}
fn task(tx: &Transaction<'_>, id: Uuid) -> Result<(ClaimedCard, Uuid, String, Option<String>)> {
    let (raw, state, reason): (String, String, Option<String>) = tx
        .query_row(
            "SELECT data,status,reason FROM cards WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db_error)?;
    let card: ClaimedCard = decode(&raw)?;
    let receipt: PrivateCodeTaskRequest = serde_json::from_value(
        card.required_capabilities
            .get(RECEIPT)
            .cloned()
            .ok_or_else(|| rejected("not a private coding task"))?,
    )
    .map_err(|_| rejected("invalid private submission"))?;
    if receipt.request_id != id
        || receipt.project_id != card.project_id
        || card.required_capabilities.get("target_node_id") != Some(&json!(receipt.target_node_id))
    {
        return Err(rejected("invalid private task target"));
    }
    Ok((card, receipt.target_node_id, state, reason))
}
impl LocalHub {
    /// Owner intent only; contains no credential, local path or arbitrary capabilities.
    pub fn private_preparation_request(
        &self,
        operation: Uuid,
        card_id: Uuid,
    ) -> Result<PreparationStatus> {
        if operation.is_nil() {
            return Err(rejected("invalid preparation identity"));
        }
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let (card, target, state, reason) = task(tx, card_id)?;
            verify_target(tx, &owner, target)?;
            let existing: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM private_preparations WHERE id=?1)", [operation.to_string()], |r| r.get(0)).map_err(db_error)?;
            if existing {
                let receipt = status(tx, operation)?;
                if receipt.task_id != card_id || receipt.target_node_id != target { return Err(rejected("preparation request ID conflicts with existing work")); }
                return Ok(receipt);
            }
            if state != "blocked" || reason.as_deref() != Some(WAITING) { return Err(rejected("task is not awaiting preparation")); }
            tx.execute("INSERT INTO private_preparations(id,card_id,target_node_id,card,state,created) VALUES(?1,?2,?3,?4,'queued',?5)", params![operation.to_string(),card_id.to_string(),target.to_string(),encode(&card)?,now()]).map_err(db_error)?;
            status(tx, operation)
        })
    }
    pub fn private_preparation_status(&self, operation: Uuid) -> Result<PreparationStatus> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let receipt = status(tx, operation)?;
            verify_target(tx, &owner, receipt.target_node_id)?;
            Ok(receipt)
        })
    }
    /// Explicit owner recovery. Retire the previous session permanently for this preparation.
    /// Files are never reset; the replacement worker must still acquire the managed host lock.
    pub fn private_preparation_recover(
        &self,
        request: Uuid,
        operation: Uuid,
    ) -> Result<PreparationStatus> {
        if request.is_nil() {
            return Err(rejected("invalid recovery request identity"));
        }
        self.with_node(|tx,node| {
            let owner=verified_owner(tx,node)?;
            let receipt=status(tx,operation)?;
            verify_target(tx,&owner,receipt.target_node_id)?;
            let existing: Option<String>=tx.query_row("SELECT operation_id FROM private_preparation_recoveries WHERE request_id=?1",[request.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            if let Some(existing)=existing {
                if existing!=operation.to_string() { return Err(rejected("recovery request conflicts with another operation")); }
                return Ok(receipt);
            }
            if receipt.state!="claimed" { return Err(rejected("only an interrupted claimed preparation can be recovered")); }
            let (card,_,state,reason)=task(tx,receipt.task_id)?;
            if state!="blocked" || reason.as_deref()!=Some(WAITING) { return Err(rejected("task is no longer awaiting preparation")); }
            let used: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM private_runs WHERE card_id=?1) OR EXISTS(SELECT 1 FROM leases WHERE card_id=?1)",[card.id.to_string()],|r|r.get(0)).map_err(db_error)?;
            if used { return Err(rejected("preparation recovery cannot change an execution attempt")); }
            let session: String=tx.query_row("SELECT session FROM private_preparations WHERE id=?1",[operation.to_string()],|r|r.get(0)).map_err(db_error)?;
            tx.execute("INSERT INTO private_preparation_recoveries VALUES(?1,?2,?3,?4,?5)",params![request.to_string(),operation.to_string(),session,node,now()]).map_err(db_error)?;
            tx.execute("UPDATE private_preparations SET state='queued',session=NULL WHERE id=?1",[operation.to_string()]).map_err(db_error)?;
            status(tx,operation)
        })
    }
    /// Same-session replay reconciles a lost response. Another session cannot silently take over.
    pub fn private_preparation_take(&self) -> Result<Option<PreparationWork>> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let target = Uuid::parse_str(node).map_err(|_| rejected("invalid node"))?;
            verify_target(tx, &owner, target)?;
            let row: Option<(String,String,String,Option<String>)> = tx.query_row("SELECT id,card,state,session FROM private_preparations WHERE target_node_id=?1 AND state!='prepared' ORDER BY created,rowid LIMIT 1", [node], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
            let Some((id, raw, state, session)) = row else { return Ok(None); };
            let retired: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM private_preparation_recoveries WHERE operation_id=?1 AND retired_session=?2)",params![id,self.session.to_string()],|r|r.get(0)).map_err(db_error)?;
            if retired { return Err(rejected("this preparation session was retired; reconnect before recovering")); }
            if state == "claimed" && session.as_deref() != Some(self.session.to_string().as_str()) { return Err(rejected("preparation belongs to another session; reconcile before retrying")); }
            let card: ClaimedCard = decode(&raw)?;
            let (current, bound, state, reason) = task(tx, card.id)?;
            if bound != target || encode(&current)? != raw || state != "blocked" || reason.as_deref() != Some(WAITING) { return Err(rejected("staged task changed before preparation")); }
            tx.execute("UPDATE private_preparations SET state='claimed',session=?2 WHERE id=?1", params![id,self.session.to_string()]).map_err(db_error)?;
            Ok(Some(PreparationWork { operation_id: Uuid::parse_str(&id).map_err(|_| rejected("invalid operation"))?, card }))
        })
    }
    /// Only the assigned worker may attest its local checkout. The controller supplies no path.
    pub fn private_preparation_complete(
        &self,
        operation: Uuid,
        workspace: &str,
    ) -> Result<PreparationStatus> {
        check_text(workspace, 4096)?;
        if !Path::new(workspace).is_absolute() {
            return Err(rejected(
                "prepared workspace must be absolute on the execution host",
            ));
        }
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let receipt = status(tx, operation)?;
            verify_target(tx, &owner, receipt.target_node_id)?;
            if receipt.target_node_id.to_string() != node {
                return Err(rejected("preparation belongs to another computer"));
            }
            let (raw, session, prior): (String, Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT card,session,workspace FROM private_preparations WHERE id=?1",
                    [operation.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(db_error)?;
            if session.as_deref() != Some(self.session.to_string().as_str()) {
                return Err(rejected("preparation session does not match"));
            }
            if receipt.state == "prepared" {
                if prior.as_deref() != Some(workspace) {
                    return Err(rejected("preparation receipt changed"));
                }
                return Ok(receipt);
            }
            if receipt.state != "claimed" {
                return Err(rejected("preparation was not claimed"));
            }
            let (mut card, _, state, reason) = task(tx, receipt.task_id)?;
            if encode(&card)? != raw || state != "blocked" || reason.as_deref() != Some(WAITING) {
                return Err(rejected("task changed during preparation"));
            }
            card.required_capabilities["prepared_workspace_root"] = json!(workspace);
            card.requires_internet = false;
            tx.execute(
                "UPDATE cards SET data=?2,reason='awaiting_private_run' WHERE id=?1",
                params![card.id.to_string(), encode(&card)?],
            )
            .map_err(db_error)?;
            tx.execute(
                "UPDATE private_preparations SET state='prepared',workspace=?2 WHERE id=?1",
                params![operation.to_string(), workspace],
            )
            .map_err(db_error)?;
            status(tx, operation)
        })
    }
}

impl RemoteLocalHub {
    /// Target-side helper. Git credentials stay on this host; only a completion path is sent.
    /// A failed acknowledgement leaves durable claimed state, never an automatic run.
    #[cfg(feature = "sandbox")]
    pub async fn prepare_next_private_checkout(
        &self,
        data: &Path,
        token: &str,
    ) -> Result<Option<PreparationStatus>> {
        let Some(work) = self.private_preparation_take().await? else {
            return Ok(None);
        };
        let spec = crate::coder::CodeSessionSpec::from_required_capabilities(
            &work.card.required_capabilities,
        )
        .map_err(|_| rejected("invalid preparation specification"))?;
        let prepared = crate::coder::workspace::prepare_authenticated(data, work.card.id, &spec, token).await.map_err(|_| rejected("checkout preparation failed on the execution computer; inspect its local workspace and Git access"))?;
        let root = prepared
            .root
            .to_str()
            .ok_or_else(|| rejected("non-UTF8 workspace path"))?
            .to_owned();
        // Hold the host lock through acknowledgement, including replay of a lost response.
        let result = self
            .private_preparation_complete(work.operation_id, &root)
            .await;
        drop(prepared);
        result.map(Some)
    }
}
