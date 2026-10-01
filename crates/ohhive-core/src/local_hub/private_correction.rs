//! Owner-requested, bounded correction branches. Neither old results nor workspaces mutate.
use super::private_code_tasks::verified_owner;
#[cfg(feature = "sandbox")]
use super::private_code_tasks::{
    agent_snapshot, stage_task, validate_review_source, verify_target, PrivateCodeTaskRequest,
    RECEIPT,
};
use super::*;
const CORRECTION_RECEIPT: &str = "__hive_private_correction_submission_v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateCorrectionRequest {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub review_task_id: Uuid,
}
impl LocalHub {
    pub fn private_code_correction_stage(
        &self,
        request: &PrivateCorrectionRequest,
    ) -> Result<ClaimedCard> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            stage(tx, request, &owner)
        })
    }
}
#[cfg(feature = "sandbox")]
fn source(
    tx: &Transaction<'_>,
    request: &PrivateCorrectionRequest,
    owner: &str,
) -> Result<(
    PrivateCodeTaskRequest,
    ClaimedCard,
    crate::coder::checker::CorrectionContext,
)> {
    use crate::coder::checker::{CorrectionContext, ReviewReceipt, ReviewRequest};
    let (raw,status,output): (String,String,Option<String>) = tx.query_row("SELECT c.data,c.status,o.content FROM cards c LEFT JOIN card_outputs o ON o.card_id=c.id WHERE c.id=?1 AND c.project_id=?2",params![request.review_task_id.to_string(),request.project_id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_error)?;
    if !["review", "done"].contains(&status.as_str()) {
        return Err(rejected(
            "independent checker must finish before correction",
        ));
    }
    let checker: ClaimedCard = decode(&raw)?;
    let review: ReviewRequest = serde_json::from_value(
        checker
            .required_capabilities
            .get("independent_review")
            .cloned()
            .ok_or_else(|| rejected("correction requires an independent checker task"))?,
    )
    .map_err(|_| rejected("invalid checker specification"))?;
    let checker_submission: PrivateCodeTaskRequest =
        serde_json::from_value(checker.required_capabilities[RECEIPT].clone())
            .map_err(|_| rejected("invalid checker submission"))?;
    verify_target(tx, owner, checker_submission.target_node_id)?;
    agent_snapshot(tx, &checker_submission)?;
    validate_review_source(tx, &checker)?;
    let receipt =
        ReviewReceipt::from_output(&output.ok_or_else(|| rejected("checker has no verdict"))?)
            .map_err(|e| rejected(&e.to_string()))?;
    receipt
        .validate(request.review_task_id, &review)
        .map_err(|e| rejected(&e.to_string()))?;
    let raw: String = tx
        .query_row(
            "SELECT data FROM cards WHERE id=?1",
            [review.package.snapshot.identity.task_id.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    let coder: ClaimedCard = decode(&raw)?;
    let original: PrivateCodeTaskRequest =
        serde_json::from_value(coder.required_capabilities[RECEIPT].clone())
            .map_err(|_| rejected("invalid original coding submission"))?;
    verify_target(tx, owner, original.target_node_id)?;
    agent_snapshot(tx, &original)?;
    let round = match coder.required_capabilities.get("checker_correction") {
        Some(value) => serde_json::from_value::<CorrectionContext>(value.clone())
            .map_err(|_| rejected("invalid correction history"))?
            .round
            .checked_add(1)
            .ok_or_else(|| rejected("correction round overflow"))?,
        None => 1,
    };
    let context = CorrectionContext {
        review_task_id: request.review_task_id,
        review,
        receipt,
        round,
    };
    context.validate().map_err(|e| rejected(&e.to_string()))?;
    Ok((original, coder, context))
}
#[cfg(feature = "sandbox")]
pub(super) fn stage(
    tx: &Transaction<'_>,
    request: &PrivateCorrectionRequest,
    owner: &str,
) -> Result<ClaimedCard> {
    if request.request_id.is_nil()
        || request.project_id.is_nil()
        || request.review_task_id.is_nil()
        || request.request_id == request.review_task_id
    {
        return Err(rejected("invalid correction request identity"));
    }
    let receipt =
        serde_json::to_value(request).map_err(|_| rejected("invalid correction request"))?;
    if let Some(raw) = tx
        .query_row(
            "SELECT data FROM cards WHERE id=?1",
            [request.request_id.to_string()],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error)?
    {
        let card: ClaimedCard = decode(&raw)?;
        if card.project_id != request.project_id
            || card.required_capabilities.get(CORRECTION_RECEIPT) != Some(&receipt)
        {
            return Err(rejected(
                "correction request ID conflicts with existing work",
            ));
        }
        let original: PrivateCodeTaskRequest =
            serde_json::from_value(card.required_capabilities[RECEIPT].clone())
                .map_err(|_| rejected("invalid correction submission"))?;
        verify_target(tx, owner, original.target_node_id)?;
        return Ok(card);
    }
    let (mut original, coder, context) = source(tx, request, owner)?;
    original.request_id = request.request_id;
    original.title = format!(
        "Correction {}: {}",
        context.round,
        coder.title.chars().take(300).collect::<String>()
    );
    let mut card = stage_task(tx, &original)?;
    let repo = coder
        .required_capabilities
        .get("repo_url")
        .and_then(Value::as_str)
        .ok_or_else(|| rejected("source repository is unavailable"))?;
    // The current profile only establishes continued eligibility. It must not replace
    // the persona and instructions frozen when the owner submitted the original job.
    for key in ["task", "__hive_private_agent_v1"] {
        card.required_capabilities[key] = coder
            .required_capabilities
            .get(key)
            .cloned()
            .ok_or_else(|| rejected("original coding persona is unavailable"))?;
    }
    card.required_capabilities["repo_url"] = json!(repo);
    card.required_capabilities["repo_ref"] = json!(context.review.package.snapshot.base_commit);
    card.required_capabilities["checker_correction"] =
        serde_json::to_value(context).map_err(|_| rejected("invalid correction context"))?;
    card.required_capabilities[CORRECTION_RECEIPT] = receipt;
    card.deps = vec![format!("private-code-{}", request.review_task_id)];
    crate::coder::CodeSessionSpec::from_required_capabilities(&card.required_capabilities)
        .map_err(|e| rejected(&e.to_string()))?;
    validate_card(&card)?;
    tx.execute(
        "UPDATE cards SET data=?2 WHERE id=?1",
        params![card.id.to_string(), encode(&card)?],
    )
    .map_err(db_error)?;
    Ok(card)
}
#[cfg(not(feature = "sandbox"))]
pub(super) fn stage(
    _tx: &Transaction<'_>,
    _request: &PrivateCorrectionRequest,
    _owner: &str,
) -> Result<ClaimedCard> {
    Err(rejected("correction requires the coding engine"))
}
/// Revalidate the immutable review result and source before preparation/claim, without
/// broadening the frozen coding persona or changing an earlier attempt.
pub(super) fn validate_source(tx: &Transaction<'_>, card: &ClaimedCard) -> Result<()> {
    let Some(receipt) = card.required_capabilities.get(CORRECTION_RECEIPT) else {
        return Ok(());
    };
    #[cfg(feature = "sandbox")]
    {
        let request: PrivateCorrectionRequest = serde_json::from_value(receipt.clone())
            .map_err(|_| rejected("invalid correction submission"))?;
        let original: PrivateCodeTaskRequest =
            serde_json::from_value(card.required_capabilities[RECEIPT].clone())
                .map_err(|_| rejected("invalid correction coder"))?;
        let owner = verified_owner(tx, &original.target_node_id.to_string())?;
        let (_, _, current) = source(tx, &request, &owner)?;
        if card.required_capabilities.get("checker_correction")
            != Some(
                &serde_json::to_value(current)
                    .map_err(|_| rejected("invalid correction source"))?,
            )
        {
            return Err(rejected(
                "correction source changed; request a fresh correction",
            ));
        }
        Ok(())
    }
    #[cfg(not(feature = "sandbox"))]
    {
        let _ = (tx, receipt);
        Err(rejected("correction requires the coding engine"))
    }
}
