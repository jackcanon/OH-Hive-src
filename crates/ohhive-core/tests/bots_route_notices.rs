#![cfg(all(feature = "bots", feature = "local-hub"))]
use hive_core::{bots::*, local_hub::LocalHubStore};
use std::sync::Arc;
use uuid::Uuid;

fn agent(
    store: &LocalHubStore,
    owner: Uuid,
    host: Option<Uuid>,
    kind: AgentRuntimeKind,
) -> AgentProfile {
    store
        .bots_agents_create(NewAgentProfile {
            owner,
            name: "Teammate".into(),
            runtime_kind: kind,
            preferred_host: host,
            capability_policy_ref: "default".into(),
            provider_account_ref: None,
            memory_namespace: Uuid::new_v4().to_string(),
        })
        .unwrap()
}
fn room(store: &LocalHubStore, a: &AgentProfile) -> Conversation {
    store
        .bots_conversations_create(NewConversation {
            title: Some("Test".into()),
            owner: a.owner,
            kind: ConversationKind::Team,
            project_id: None,
            coordinator: Some(a.id),
            storage_scope: StorageScope::LocalOnly,
        })
        .unwrap()
}
fn send(store: &LocalHubStore, c: &Conversation, a: &AgentProfile) -> Message {
    store
        .bots_message_send(
            Principal::User(c.owner),
            c.id,
            Uuid::new_v4().to_string(),
            1,
            vec![a.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("Please reply".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
        )
        .unwrap()
}
fn messages(store: &LocalHubStore, c: &Conversation) -> Vec<Message> {
    store
        .bots_messages_list(
            Principal::User(c.owner),
            c.id,
            MessagePage {
                before: None,
                after: None,
                limit: 200,
            },
        )
        .unwrap()
}

#[test]
fn notices_are_scoped_deduplicated_and_leave_recoverable_work() {
    let store = LocalHubStore::in_memory().unwrap();
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let a = agent(&store, owner, None, AgentRuntimeKind::NousByok);
    let c = room(&store, &a);
    let m = send(&store, &c, &a);
    let foreign = agent(
        &store,
        Uuid::new_v4(),
        None,
        AgentRuntimeKind::AnthropicByok,
    );
    let f = room(&store, &foreign);
    send(&store, &f, &foreign);
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 1);
    assert_eq!(store.bots_report_unroutable(owner, host, false).unwrap(), 0);
    let history = messages(&store, &c);
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].kind, MessageKind::System);
    assert_eq!(history[1].thread_root, Some(m.id));
    assert!(history[1]
        .body
        .as_ref()
        .unwrap()
        .contains("not implemented"));
    assert_eq!(messages(&store, &f).len(), 1);
    let pending = store.bots_deliveries_pending_for_agent(a.id, 200).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].key.message_id, m.id);
    // A future runner can claim the original delivery; the diagnostic did not terminalize it.
    store.bots_delivery_claim(pending[0].key).unwrap();
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 0);
}
#[test]
fn local_setup_and_other_hosts_are_not_confused_with_offline_detection() {
    let store = LocalHubStore::in_memory().unwrap();
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let here = agent(&store, owner, Some(host), AgentRuntimeKind::Local);
    let c = room(&store, &here);
    send(&store, &c, &here);
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 0);
    assert_eq!(store.bots_report_unroutable(owner, host, false).unwrap(), 1);
    assert!(messages(&store, &c)[1]
        .body
        .as_ref()
        .unwrap()
        .contains("model settings"));
    let there = agent(&store, owner, Some(Uuid::new_v4()), AgentRuntimeKind::Local);
    let r = room(&store, &there);
    send(&store, &r, &there);
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 1);
    assert!(messages(&store, &r)[1]
        .body
        .as_ref()
        .unwrap()
        .contains("availability has not been verified"));
}
#[test]
fn concurrent_reporters_post_only_once_and_running_work_is_untouched() {
    let store = Arc::new(LocalHubStore::in_memory().unwrap());
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let a = agent(&store, owner, None, AgentRuntimeKind::ChatgptSubscription);
    let c = room(&store, &a);
    send(&store, &c, &a);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let s = store.clone();
            std::thread::spawn(move || s.bots_report_unroutable(owner, host, true).unwrap())
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .sum::<usize>(),
        1
    );
    let m = send(&store, &c, &a);
    store
        .bots_delivery_claim(DeliveryKey {
            message_id: m.id,
            recipient: a.id,
        })
        .unwrap();
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 0);
}
#[test]
fn bounded_reporting_progresses_past_already_reported_rows() {
    let store = LocalHubStore::in_memory().unwrap();
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let a = agent(&store, owner, None, AgentRuntimeKind::AnthropicByok);
    let c = room(&store, &a);
    for _ in 0..101 {
        send(&store, &c, &a);
    }
    assert_eq!(
        store.bots_report_unroutable(owner, host, true).unwrap(),
        100
    );
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 1);
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 0);
    assert_eq!(
        store
            .bots_deliveries_pending_for_agent(a.id, 200)
            .unwrap()
            .len(),
        101
    );
}
struct MustNotRun;
#[async_trait::async_trait]
impl LocalBotsTurnRunner for MustNotRun {
    async fn run_turn(
        &self,
        _: &AgentProfile,
        _: LocalTurnRequest,
    ) -> Result<LocalTurnOutcome, LocalTurnError> {
        panic!("Unsupported runtime must not reach the local model")
    }
}
#[tokio::test]
async fn production_executor_reports_unsupported_runtime_without_running_it() {
    let store = Arc::new(LocalHubStore::in_memory().unwrap());
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let a = agent(&store, owner, None, AgentRuntimeKind::NousByok);
    let c = room(&store, &a);
    send(&store, &c, &a);
    let executor = DeliveryExecutor::new(store.clone(), Arc::new(MustNotRun), host, owner);
    assert_eq!(executor.drain_once().await.delivered, 0);
    executor.drain_once().await;
    assert_eq!(messages(&store, &c).len(), 2);
}

#[test]
fn held_and_finished_work_never_get_route_notices() {
    let store = LocalHubStore::in_memory().unwrap();
    let owner = Uuid::new_v4();
    let host = Uuid::new_v4();
    let a = agent(&store, owner, None, AgentRuntimeKind::NousByok);
    let c = room(&store, &a);
    store
        .bots_message_send_with_cause(
            Principal::User(owner),
            c.id,
            "held".into(),
            1,
            vec![a.id],
            NewMessage {
                thread_root: None,
                kind: MessageKind::Text,
                body: Some("Held".into()),
                attachment_refs: vec![],
                task_ref: None,
                turn_ref: None,
                source_event_ref: None,
            },
            None,
            true,
        )
        .unwrap();
    let m = send(&store, &c, &a);
    let key = DeliveryKey {
        message_id: m.id,
        recipient: a.id,
    };
    let claim = store.bots_delivery_claim(key).unwrap();
    store
        .bots_delivery_complete(key, claim.lease_generation)
        .unwrap();
    assert_eq!(store.bots_report_unroutable(owner, host, true).unwrap(), 0);
    assert_eq!(messages(&store, &c).len(), 2);
}
