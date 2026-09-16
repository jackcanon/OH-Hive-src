use super::*;
use crate::subscription::{
    journal::Journal,
    results::DurableResults,
    runner::{ProviderReply, ResultStore},
};
use std::{path::PathBuf, sync::Arc};
struct Fixture {
    dir: PathBuf,
    store: Arc<LocalHubStore>,
    binding: Binding,
    operation: Uuid,
    generation: u64,
    results: DurableResults,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Fixture {
    async fn new(complete: bool) -> Self {
        let dir = std::env::temp_dir().join(format!("hive-publish-{}", Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let store = Arc::new(LocalHubStore::open(dir.join("hub.db")).unwrap());
        let owner = Uuid::new_v4();
        let host = Uuid::new_v4();
        let account = Uuid::new_v4();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Copilot".into(),
                runtime_kind: AgentRuntimeKind::CopilotSubscription,
                preferred_host: Some(host),
                capability_policy_ref: "default".into(),
                provider_account_ref: Some(account),
                memory_namespace: "test".into(),
            })
            .unwrap();
        let room = store
            .bots_conversations_create(NewConversation {
                title: None,
                owner,
                kind: ConversationKind::AgentDm,
                project_id: None,
                coordinator: Some(agent.id),
                storage_scope: StorageScope::LocalOnly,
            })
            .unwrap();
        store
            .bots_conversations_join(Principal::Agent(agent.id), room.id)
            .unwrap();
        store
            .bots_conversations_join(Principal::User(owner), room.id)
            .unwrap();
        let incoming = store
            .bots_message_send(
                Principal::User(owner),
                room.id,
                "input".into(),
                room.policy_revision,
                vec![agent.id],
                NewMessage {
                    thread_root: None,
                    kind: MessageKind::Text,
                    body: Some("hello".into()),
                    attachment_refs: vec![],
                    task_ref: None,
                    turn_ref: None,
                    source_event_ref: None,
                },
            )
            .unwrap();
        let delivery = store
            .bots_delivery_claim(DeliveryKey {
                message_id: incoming.id,
                recipient: agent.id,
            })
            .unwrap();
        let binding = Binding {
            session: Uuid::new_v4(),
            owner,
            host,
            agent: agent.id,
            conversation: room.id,
            account,
            provider: Provider::Copilot,
            workspace: Uuid::new_v4(),
            policy_revision: room.policy_revision.to_string(),
        };
        let mut journal = Journal::open(&dir.join("journal.db")).unwrap();
        let results = DurableResults::open(&dir.join("journal.db")).unwrap();
        let lease = journal.acquire(&binding, Uuid::new_v4(), 30).unwrap();
        journal
            .prepare(&lease, incoming.id, &"a".repeat(64))
            .unwrap();
        journal.mark_dispatched(&lease, incoming.id).unwrap();
        journal
            .acknowledge(&lease, incoming.id, "provider-turn")
            .unwrap();
        let receipt = results
            .persist(
                &binding,
                incoming.id,
                &ProviderReply {
                    turn_id: "provider-turn".into(),
                    text: "Saved answer".into(),
                },
            )
            .await
            .unwrap();
        if complete {
            journal
                .finish(&lease, incoming.id, "provider-turn", &receipt, true)
                .unwrap();
        }
        journal.release(&lease).unwrap();
        Self {
            dir,
            store,
            binding,
            operation: incoming.id,
            generation: delivery.lease_generation,
            results,
        }
    }
    async fn publish(&self) -> std::result::Result<Uuid, crate::subscription::runner::RunError> {
        self.results
            .publish_to_bots(
                self.store.clone(),
                &self.binding,
                self.operation,
                self.generation,
            )
            .await
    }
    fn count(&self) -> i64 {
        self.store
            .transaction(|tx| {
                tx.query_row("SELECT count(*) FROM messages", [], |r| r.get(0))
                    .map_err(db_error)
            })
            .unwrap()
    }
    fn status(&self) -> String {
        self.store
            .transaction(|tx| {
                tx.query_row("SELECT status FROM agent_deliveries", [], |r| r.get(0))
                    .map_err(db_error)
            })
            .unwrap()
    }
}
#[tokio::test]
async fn publication_reopens_replays_and_finishes_delivery_atomically() {
    let f = Fixture::new(true).await;
    let id = f.publish().await.unwrap();
    assert_eq!(f.count(), 2);
    assert_eq!(f.status(), "done");
    let hub = Arc::new(LocalHubStore::open(f.dir.join("hub.db")).unwrap());
    let results = DurableResults::open(&f.dir.join("journal.db")).unwrap();
    assert_eq!(
        results
            .publish_to_bots(hub, &f.binding, f.operation, f.generation)
            .await
            .unwrap(),
        id
    );
    let message = f.store.bots_message_get(id).unwrap();
    assert_eq!(message.body.as_deref(), Some("Saved answer"));
    assert_eq!(message.author, Principal::Agent(f.binding.agent));
    assert_eq!(f.count(), 2);
}
#[tokio::test]
async fn failed_delivery_update_rolls_back_message_then_recovers() {
    let f = Fixture::new(true).await;
    f.store.transaction(|tx|tx.execute_batch("CREATE TRIGGER fail_publish BEFORE UPDATE ON agent_deliveries BEGIN SELECT RAISE(ABORT,'injected failure'); END;").map_err(db_error)).unwrap();
    assert!(f.publish().await.is_err());
    assert_eq!(f.count(), 1);
    assert_eq!(f.status(), "running");
    f.store
        .transaction(|tx| {
            tx.execute_batch("DROP TRIGGER fail_publish;")
                .map_err(db_error)
        })
        .unwrap();
    f.publish().await.unwrap();
    assert_eq!(f.count(), 2);
    assert_eq!(f.status(), "done");
}
#[tokio::test]
async fn rejects_unfinished_stale_cancelled_and_changed_permissions() {
    let f = Fixture::new(false).await;
    assert!(f.publish().await.is_err());
    assert_eq!(f.count(), 1);
    let mut f = Fixture::new(true).await;
    f.generation += 1;
    assert!(f.publish().await.is_err());
    f.generation -= 1;
    f.store
        .transaction(|tx| {
            tx.execute(
                "UPDATE conversation_members SET allowed_actions='[]' WHERE principal_kind='agent'",
                [],
            )
            .map(|_| ())
            .map_err(db_error)
        })
        .unwrap();
    assert!(f.publish().await.is_err());
    assert_eq!(f.count(), 1);
    let f = Fixture::new(true).await;
    f.store
        .transaction(|tx| {
            tx.execute("UPDATE agent_deliveries SET status='cancelled'", [])
                .map(|_| ())
                .map_err(db_error)
        })
        .unwrap();
    assert!(f.publish().await.is_err());
    assert_eq!(f.count(), 1);
}
#[tokio::test]
async fn rejects_changed_account_and_conflicting_existing_reply() {
    let f = Fixture::new(true).await;
    f.store
        .transaction(|tx| {
            tx.execute(
                "UPDATE agent_profiles SET provider_account_ref=?1",
                params![Uuid::new_v4().to_string()],
            )
            .map(|_| ())
            .map_err(db_error)
        })
        .unwrap();
    assert!(f.publish().await.is_err());
    assert_eq!(f.count(), 1);
    let f = Fixture::new(true).await;
    let id = f.publish().await.unwrap();
    f.store
        .transaction(|tx| {
            tx.execute(
                "UPDATE messages SET body='conflict' WHERE id=?1",
                params![id.to_string()],
            )
            .map(|_| ())
            .map_err(db_error)
        })
        .unwrap();
    assert!(f.publish().await.is_err());
    assert_eq!(f.count(), 2);
}
