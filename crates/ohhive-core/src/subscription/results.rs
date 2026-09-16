//! Private host-local reply storage. This is not a Bots conversation publisher.
//! Open the same database as Journal; results and operation bindings share a schema.
use super::{
    journal::{Binding, Journal},
    runner::{ProviderReply, ResultStore, RunError},
};
use async_trait::async_trait;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct DurableResults(Arc<Mutex<Journal>>);
impl DurableResults {
    pub fn open(path: &Path) -> Result<Self, RunError> {
        Ok(Self(Arc::new(Mutex::new(Journal::open(path)?))))
    }
    fn access(
        &self,
        binding: &Binding,
        operation: Uuid,
        reply: Option<&ProviderReply>,
    ) -> Result<Option<(ProviderReply, String)>, RunError> {
        let fail = |_| RunError::Persistence;
        let mut journal = self.0.lock().map_err(fail)?;
        let tx = journal
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| RunError::Persistence)?;
        let session = binding.session.to_string();
        let operation = operation.to_string();
        let stored: Option<(String, String, Option<String>)> = tx.query_row(
            "SELECT s.binding,t.state,t.provider_turn FROM sessions s JOIN turns t ON t.session=s.id WHERE s.id=?1 AND t.operation=?2",
            params![session,operation], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|_| RunError::Persistence)?;
        let (encoded, state, known_turn) = stored.ok_or(RunError::Persistence)?;
        if encoded != serde_json::to_string(binding).map_err(|_| RunError::Invalid)? {
            return Err(RunError::Persistence);
        }
        let existing: Option<(ProviderReply, String)> = tx
            .query_row(
                "SELECT provider_turn,body,receipt FROM results WHERE session=?1 AND operation=?2",
                params![session, operation],
                |r| {
                    Ok((
                        ProviderReply {
                            turn_id: r.get(0)?,
                            text: r.get(1)?,
                        },
                        r.get(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| RunError::Persistence)?;
        if let Some(reply) = reply {
            if reply.turn_id.is_empty()
                || reply.turn_id.len() > 256
                || reply.text.is_empty()
                || reply.text.len() > 256 * 1024
            {
                return Err(RunError::Invalid);
            }
            if known_turn.as_deref() != Some(reply.turn_id.as_str()) {
                return Err(RunError::Persistence);
            }
            if let Some((old, _)) = &existing {
                if old.turn_id != reply.turn_id || old.text != reply.text {
                    return Err(RunError::Persistence);
                }
            } else {
                if state != "dispatched" && state != "delivery_unknown" {
                    return Err(RunError::Persistence);
                }
                let receipt = Uuid::new_v4().to_string();
                tx.execute("INSERT INTO results(session,operation,provider_turn,body,receipt) VALUES(?1,?2,?3,?4,?5)",params![session,operation,reply.turn_id,reply.text,receipt]).map_err(|_| RunError::Persistence)?;
                tx.commit().map_err(|_| RunError::Persistence)?;
                return Ok(Some((
                    ProviderReply {
                        turn_id: reply.turn_id.clone(),
                        text: reply.text.clone(),
                    },
                    receipt,
                )));
            }
        }
        tx.commit().map_err(|_| RunError::Persistence)?;
        Ok(existing)
    }
}
#[async_trait]
impl ResultStore for DurableResults {
    async fn persist(
        &self,
        binding: &Binding,
        operation: Uuid,
        reply: &ProviderReply,
    ) -> Result<String, RunError> {
        let store = self.clone();
        let binding = binding.clone();
        let reply = ProviderReply {
            turn_id: reply.turn_id.clone(),
            text: reply.text.clone(),
        };
        tokio::task::spawn_blocking(move || {
            store
                .access(&binding, operation, Some(&reply))?
                .map(|(_, receipt)| receipt)
                .ok_or(RunError::Persistence)
        })
        .await
        .map_err(|_| RunError::Persistence)?
    }
    async fn load(
        &self,
        binding: &Binding,
        operation: Uuid,
    ) -> Result<Option<ProviderReply>, RunError> {
        let store = self.clone();
        let binding = binding.clone();
        tokio::task::spawn_blocking(move || {
            store
                .access(&binding, operation, None)
                .map(|result| result.map(|(reply, _)| reply))
        })
        .await
        .map_err(|_| RunError::Persistence)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subscription::{
        journal::Provider,
        runner::{Envelope, SubscriptionRuntime, TurnRunner},
    };
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("hive-results-{}.db", Uuid::new_v4())))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    struct Offline;
    #[async_trait]
    impl SubscriptionRuntime for Offline {
        fn provider(&self) -> Provider {
            Provider::Copilot
        }
        async fn send(
            &self,
            _: &Binding,
            _: Uuid,
            _: &Envelope,
        ) -> Result<ProviderReply, RunError> {
            panic!("must not resend")
        }
        async fn reconcile(
            &self,
            _: &Binding,
            _: Uuid,
            _: Option<&str>,
        ) -> Result<Option<ProviderReply>, RunError> {
            panic!("saved reply must recover offline")
        }
        async fn interrupt(&self, _: &Binding, _: Uuid) {}
    }
    fn binding() -> Binding {
        Binding {
            session: Uuid::new_v4(),
            owner: Uuid::new_v4(),
            host: Uuid::new_v4(),
            agent: Uuid::new_v4(),
            conversation: Uuid::new_v4(),
            account: Uuid::new_v4(),
            provider: Provider::Copilot,
            workspace: Uuid::new_v4(),
            policy_revision: "1".into(),
        }
    }
    #[tokio::test]
    async fn saved_reply_recovers_after_restart_without_provider_and_rejects_conflicts() {
        let temp = Temp::new();
        let path = &temp.0;
        let binding = binding();
        let operation = Uuid::new_v4();
        let mut journal = Journal::open(path).unwrap();
        let lease = journal.acquire(&binding, Uuid::new_v4(), 30).unwrap();
        journal.prepare(&lease, operation, &"a".repeat(64)).unwrap();
        journal.mark_dispatched(&lease, operation).unwrap();
        journal.acknowledge(&lease, operation, "turn-1").unwrap();
        let store = DurableResults::open(path).unwrap();
        let reply = ProviderReply {
            turn_id: "turn-1".into(),
            text: "durable answer".into(),
        };
        let receipt = store.persist(&binding, operation, &reply).await.unwrap();
        assert_eq!(
            receipt,
            store.persist(&binding, operation, &reply).await.unwrap()
        );
        assert!(store
            .persist(
                &binding,
                operation,
                &ProviderReply {
                    turn_id: "turn-1".into(),
                    text: "different".into()
                }
            )
            .await
            .is_err());
        let mut other = binding.clone();
        other.owner = Uuid::new_v4();
        assert!(store.load(&other, operation).await.is_err());
        journal.release(&lease).unwrap();
        drop(store);
        drop(journal);
        let mut journal = Journal::open(path).unwrap();
        let store = DurableResults::open(path).unwrap();
        assert_eq!(
            store.load(&binding, operation).await.unwrap().unwrap().text,
            "durable answer"
        );
        let runner = TurnRunner {
            runtime: Offline,
            results: store,
        };
        let (_sender, cancel) = tokio::sync::watch::channel(false);
        let done = runner
            .reconcile(&mut journal, &binding, operation, true, cancel)
            .await
            .unwrap();
        assert_eq!(done.receipt, receipt);
        let lease = journal.acquire(&binding, Uuid::new_v4(), 30).unwrap();
        assert_eq!(
            journal
                .prepare(&lease, operation, &"a".repeat(64))
                .unwrap()
                .state,
            "completed"
        );
    }
    #[test]
    fn migrates_v1_without_losing_turns() {
        let temp = Temp::new();
        let path = &temp.0;
        let mut journal = Journal::open(path).unwrap();
        let binding = binding();
        let operation = Uuid::new_v4();
        let lease = journal.acquire(&binding, Uuid::new_v4(), 30).unwrap();
        journal.prepare(&lease, operation, &"a".repeat(64)).unwrap();
        journal.release(&lease).unwrap();
        journal
            .connection
            .execute_batch("DROP TABLE results; PRAGMA user_version=1;")
            .unwrap();
        drop(journal);
        let mut journal = Journal::open(path).unwrap();
        let lease = journal.acquire(&binding, Uuid::new_v4(), 30).unwrap();
        assert_eq!(journal.pending(&lease).unwrap()[0].operation, operation);
        assert_eq!(
            journal
                .connection
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}
