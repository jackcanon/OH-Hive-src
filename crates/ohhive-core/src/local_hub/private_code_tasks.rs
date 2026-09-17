//! Trusted host staging for private coding jobs. No token or caller-supplied capability JSON.
use super::*;

const RECEIPT: &str = "__hive_private_submission_v1";
const WAITING: &str = "awaiting_repository_preparation";

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
