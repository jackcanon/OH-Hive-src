//! Trusted host staging for private coding jobs. No token or caller-supplied capability JSON.
use super::*;

const RECEIPT: &str = "__hive_private_submission_v1";
const WAITING: &str = "awaiting_repository_preparation";

#[derive(Clone, Serialize, Deserialize)]
pub struct PrivateCodeTaskStatus {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub reason: Option<String>,
    pub workspace: Option<String>,
    pub output: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateCodeTaskRequest {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub target_node_id: Uuid,
    pub title: String,
    pub task: String,
    pub model_id: Option<String>,
    pub max_turns: u32,
    pub acceptance: Vec<crate::acceptance::AcceptanceCheck>,
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
                result.push(PrivateCodeTaskStatus { id: card.id, title: card.title, status, reason, output, workspace: card.required_capabilities.get("prepared_workspace_root").and_then(Value::as_str).map(str::to_owned) });
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

    /// Owner-approved administration only; NOT exposed through paired-worker RPC.
    /// Stages a non-claimable card. A future host preparation operation must verify its
    /// workspace before publishing it to the runnable queue. Same request ID + input returns
    /// the original frozen card even after the project repository changes or is disconnected.
    pub fn stage_private_code_task(&self, request: &PrivateCodeTaskRequest) -> Result<ClaimedCard> {
        if request.request_id.is_nil()
            || request.project_id.is_nil()
            || request.target_node_id.is_nil()
            || request.max_turns == 0
            || request.max_turns > 100
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
        self.transaction(|tx| {
            let existing: Option<String> = tx.query_row("SELECT data FROM cards WHERE id=?1", [request.request_id.to_string()], |r| r.get(0)).optional().map_err(db_error)?;
            if let Some(raw) = existing {
                let card: ClaimedCard = decode(&raw)?;
                if card.required_capabilities.get(RECEIPT) != Some(&receipt) || card.project_id != request.project_id {
                    return Err(rejected("submission request ID conflicts with existing work"));
                }
                return Ok(card);
            }
            let target: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM nodes n JOIN local_node_keys k ON k.node_id=n.id WHERE n.id=?1 AND k.revoked=0)", [request.target_node_id.to_string()], |r| r.get(0)).map_err(db_error)?;
            if !target { return Err(rejected("target computer is not enrolled or has been revoked")); }
            let mut card = ClaimedCard {
                id: request.request_id,
                project_id: request.project_id,
                key: format!("private-code-{}", request.request_id),
                title: request.title.clone(),
                modality: "code".into(),
                inputs: request.task.clone(),
                acceptance: "Run the owner's explicit acceptance checks; otherwise report unverified.".into(),
                deps: vec![], requires_internet: true,
                required_capabilities: json!({
                    "brain":"local", "task":request.task, "max_turns":request.max_turns,
                    "model_id":request.model_id, "target_node_id":request.target_node_id,
                    "acceptance":request.acceptance, (RECEIPT):receipt
                }),
            };
            repository::apply_project_default(tx, &mut card)?;
            if card.required_capabilities.get("repo_url").and_then(Value::as_str).is_none() {
                return Err(rejected("connect a project repository before staging a coding task"));
            }
            validate_card(&card)?;
            tx.execute("INSERT INTO cards(id,project_id,key,data,status,reason) VALUES(?1,?2,?3,?4,'blocked',?5)", params![card.id.to_string(), card.project_id.to_string(), card.key, encode(&card)?, WAITING]).map_err(db_error)?;
            Ok(card)
        })
    }
}

#[cfg(feature = "sandbox")]
fn preparation_card(tx: &Transaction<'_>, id: Uuid, node: Uuid) -> Result<ClaimedCard> {
    let (raw, status, reason): (String, String, Option<String>) = tx
        .query_row(
            "SELECT data,status,reason FROM cards WHERE id=?1",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db_error)?;
    if !(status == "ready" || (status == "blocked" && reason.as_deref() == Some(WAITING))) {
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
    if status == "ready"
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
