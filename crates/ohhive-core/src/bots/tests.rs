//! C0 tests: this slice has no storage backend to exercise, so these are the pure parts --
//! serialization shape (the wire contract other implementations/clients will rely on) and the
//! small domain logic that doesn't need a database (terminal-state checks, dedup keys, the
//! section 6 loop-prevention defaults). Self-verified only via the same brace/paren/bracket
//! balance check used for `subscription/tests.rs` -- not yet compiler-verified (no Rust
//! toolchain in this sandbox).

use chrono::{TimeZone, Utc};
use uuid::Uuid;

use super::service::{MessagePage, SearchScope};
use super::types::{
    AgentRuntimeKind, ConversationKind, DeliveryKey, DeliveryStatus, Handoff, HandoffBudgets,
    HandoffState, MessageKind, Principal, RevisionKind,
};

fn fixed_time() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 15, 0, 0, 0).unwrap()
}

#[test]
fn handoff_budgets_default_matches_section_6_numbers() {
    // "one active turn per agent, one coordinator per room, two active specialist handoffs
    // per run, two correction rounds, and depth two" -- the coordinator-per-room rule lives on
    // `Conversation::coordinator` (a single `Option<AgentId>`), not here.
    let budgets = HandoffBudgets::default();
    assert_eq!(budgets.max_active_turns_per_agent, 1);
    assert_eq!(budgets.max_active_specialist_handoffs_per_run, 2);
    assert_eq!(budgets.max_correction_rounds, 2);
    assert_eq!(budgets.max_depth, 2);
    assert_eq!(budgets.max_followups, 2);
}

#[test]
fn delivery_status_terminal_states_are_exactly_done_failed_cancelled() {
    assert!(!DeliveryStatus::Pending.is_terminal());
    assert!(!DeliveryStatus::Running.is_terminal());
    assert!(!DeliveryStatus::Unknown.is_terminal());
    assert!(DeliveryStatus::Done.is_terminal());
    assert!(DeliveryStatus::Failed.is_terminal());
    assert!(DeliveryStatus::Cancelled.is_terminal());
}

#[test]
fn handoff_state_terminal_states_match_section_6_outcomes() {
    assert!(!HandoffState::Requested.is_terminal());
    assert!(!HandoffState::Accepted.is_terminal());
    assert!(!HandoffState::InProgress.is_terminal());
    assert!(!HandoffState::AwaitingCorrection.is_terminal());
    assert!(HandoffState::Rejected.is_terminal());
    assert!(HandoffState::Completed.is_terminal());
    assert!(HandoffState::Failed.is_terminal());
    assert!(HandoffState::Expired.is_terminal());
}

#[test]
fn delivery_key_is_the_message_recipient_unique_key() {
    // Section 5: "message+recipient unique key." Same (message, recipient) pair must compare
    // equal and hash equal regardless of where the pair came from -- this is what a storage
    // layer's uniqueness constraint would key on.
    let message_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let recipient = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();

    let a = DeliveryKey {
        message_id,
        recipient,
    };
    let b = DeliveryKey {
        message_id,
        recipient,
    };
    assert_eq!(a, b);

    let mut set = std::collections::HashSet::new();
    set.insert(a);
    set.insert(b);
    assert_eq!(set.len(), 1, "identical (message, recipient) must dedupe");

    let different_recipient = DeliveryKey {
        message_id,
        recipient: Uuid::parse_str("00000000-0000-0000-0000-000000000003").unwrap(),
    };
    set.insert(different_recipient);
    assert_eq!(set.len(), 2);
}

#[test]
fn handoff_dedup_key_folds_in_the_workflow_step() {
    // Section 6: "deduplicate by source request + target + workflow step." Same handoff id +
    // target + step must produce the same key; changing any one of them must not.
    let handoff = Handoff {
        id: Uuid::parse_str("00000000-0000-0000-0000-0000000000aa").unwrap(),
        source_agent: Uuid::parse_str("00000000-0000-0000-0000-0000000000bb").unwrap(),
        target_agent: Uuid::parse_str("00000000-0000-0000-0000-0000000000cc").unwrap(),
        project_id: None,
        task_or_question: "review the PR".to_string(),
        acceptance_criteria: "tests pass".to_string(),
        artifact_refs: vec![],
        allowed_tools: vec![],
        parent_run: None,
        reply_to_thread: None,
        budgets: HandoffBudgets::default(),
        deadline: fixed_time(),
        depth: 0,
        state: HandoffState::Requested,
        receipt: None,
        created_at: fixed_time(),
    };

    let key_a = handoff.dedup_key("card-42/review");
    let key_b = handoff.dedup_key("card-42/review");
    assert_eq!(key_a, key_b);

    let key_different_step = handoff.dedup_key("card-42/correction-1");
    assert_ne!(key_a, key_different_step);
}

#[test]
fn principal_serializes_with_an_explicit_kind_tag() {
    // The wire shape matters here: a client (or another agent) must be able to tell a user
    // apart from an agent without out-of-band context, since author identity is what section 5
    // says the server derives and no one else may forge.
    let user_id = Uuid::parse_str("00000000-0000-0000-0000-000000000010").unwrap();
    let principal = Principal::User(user_id);
    let json = serde_json::to_value(&principal).unwrap();
    assert_eq!(json["kind"], "user");
    assert_eq!(json["id"], user_id.to_string());

    let agent_id = Uuid::parse_str("00000000-0000-0000-0000-000000000011").unwrap();
    let principal = Principal::Agent(agent_id);
    let json = serde_json::to_value(&principal).unwrap();
    assert_eq!(json["kind"], "agent");
    assert_eq!(json["id"], agent_id.to_string());

    let round_tripped: Principal = serde_json::from_value(json).unwrap();
    assert_eq!(round_tripped, Principal::Agent(agent_id));
}

#[test]
fn conversation_kind_and_message_kind_use_snake_case_on_the_wire() {
    assert_eq!(
        serde_json::to_value(ConversationKind::AgentDm).unwrap(),
        serde_json::json!("agent_dm")
    );
    assert_eq!(
        serde_json::to_value(MessageKind::TaskReceipt).unwrap(),
        serde_json::json!("task_receipt")
    );
    assert_eq!(
        serde_json::to_value(AgentRuntimeKind::ChatgptSubscription).unwrap(),
        serde_json::json!("chatgpt_subscription")
    );
}

#[test]
fn revision_kind_replacement_carries_its_new_body_tombstone_does_not() {
    let replacement = RevisionKind::Replacement {
        new_body: "corrected text".to_string(),
    };
    let json = serde_json::to_value(&replacement).unwrap();
    assert_eq!(json["kind"], "replacement");
    assert_eq!(json["new_body"], "corrected text");

    let tombstone = RevisionKind::Tombstone;
    let json = serde_json::to_value(&tombstone).unwrap();
    assert_eq!(json["kind"], "tombstone");

    let round_tripped: RevisionKind = serde_json::from_value(json).unwrap();
    assert_eq!(round_tripped, RevisionKind::Tombstone);
}

#[test]
fn search_scope_everything_has_no_id_conversation_and_project_do() {
    let everything = serde_json::to_value(SearchScope::Everything).unwrap();
    assert_eq!(everything["kind"], "everything");

    let conversation_id = Uuid::parse_str("00000000-0000-0000-0000-000000000020").unwrap();
    let scoped = serde_json::to_value(SearchScope::Conversation(conversation_id)).unwrap();
    assert_eq!(scoped["kind"], "conversation");
    assert_eq!(scoped["id"], conversation_id.to_string());
}

#[test]
fn message_page_default_has_no_bounds_and_zero_limit() {
    // A `BotsService` implementation treats `limit: 0` as "caller must specify a page size,"
    // not as "return nothing" -- documented here so the default doesn't get mistaken for a
    // usable request.
    let page = MessagePage::default();
    assert_eq!(page.before, None);
    assert_eq!(page.after, None);
    assert_eq!(page.limit, 0);
}
