//! Shared, tool-free subscription lifecycle. No account lookup, UI, or provider
//! implementation here. Trusted callers bind an authorized cloud-enabled room.
//! The adapter must not automatically retry a provider request. The result store
//! must durably deduplicate writes by (binding.session, operation).
use super::journal::{Binding, Journal, JournalError, Lease, Provider};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub model: String,
    /// Exact authorized, bounded context for this turn; never reconstructed on retry.
    pub prompt: String,
}
impl Envelope {
    fn hash(&self) -> Result<String, RunError> {
        if self.model.trim().is_empty()
            || self.model.len() > 256
            || self.prompt.trim().is_empty()
            || self.prompt.len() > 128 * 1024
        {
            return Err(RunError::Invalid);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| RunError::Invalid)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}
pub struct ProviderReply {
    pub turn_id: String,
    pub text: String,
}
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error("Cloud coordination is not enabled for this binding")]
    CloudDisabled,
    #[error("Invalid subscription turn request")]
    Invalid,
    #[error("Subscription provider does not match the binding")]
    WrongProvider,
    #[error("Subscription turn was cancelled; sent work may require reconciliation")]
    Cancelled,
    #[error("Subscription turn timed out; do not automatically retry")]
    Timeout,
    #[error("Subscription delivery is unknown and requires reconciliation")]
    Unknown,
    #[error("Subscription result could not be persisted; reconcile before retrying")]
    Persistence,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub receipt: String,
    pub replayed: bool,
}

#[async_trait]
pub trait SubscriptionRuntime: Send + Sync {
    fn provider(&self) -> Provider;
    /// Use the stable operation ID for provider-side correlation. No tools until
    /// the scoped broker is wired. Dropping this future is NOT proof of cancellation.
    async fn send(
        &self,
        binding: &Binding,
        operation: Uuid,
        envelope: &Envelope,
    ) -> Result<ProviderReply, RunError>;
    /// Read-only recovery. None means unknown, never permission to resend.
    async fn reconcile(
        &self,
        binding: &Binding,
        operation: Uuid,
        provider_turn: Option<&str>,
    ) -> Result<Option<ProviderReply>, RunError>;
    /// Best effort provider interrupt; callers keep the journal uncertain either way.
    async fn interrupt(&self, binding: &Binding, operation: Uuid);
}
#[async_trait]
pub trait ResultStore: Send + Sync {
    /// Recover an already durable reply without asking the provider again.
    async fn load(
        &self,
        _binding: &Binding,
        _operation: Uuid,
    ) -> Result<Option<ProviderReply>, RunError> {
        Ok(None)
    }
    /// Commit once by session/operation; identical replay returns the same reference,
    /// conflicting contents must fail. Receipt must be nonempty and <=256 bytes.
    async fn persist(
        &self,
        binding: &Binding,
        operation: Uuid,
        reply: &ProviderReply,
    ) -> Result<String, RunError>;
}

pub struct TurnRunner<R, S> {
    pub runtime: R,
    pub results: S,
}
impl<R: SubscriptionRuntime, S: ResultStore> TurnRunner<R, S> {
    /// Explicit cloud consent and binding checks precede all journal/provider work.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        journal: &mut Journal,
        binding: &Binding,
        operation: Uuid,
        envelope: &Envelope,
        cloud_enabled: bool,
        cancel: watch::Receiver<bool>,
        timeout: Duration,
    ) -> Result<Completion, RunError> {
        self.validate(binding, cloud_enabled, &cancel, timeout)?;
        let hash = envelope.hash()?;
        let lease = journal.acquire(binding, Uuid::new_v4(), 30)?;
        let result = self
            .run_claimed(
                journal, &lease, binding, operation, envelope, &hash, cancel, timeout,
            )
            .await;
        let released = journal.release(&lease);
        match result {
            Ok(done) => {
                released?;
                Ok(done)
            }
            Err(error) => Err(error),
        }
    }
    fn validate(
        &self,
        binding: &Binding,
        cloud_enabled: bool,
        cancel: &watch::Receiver<bool>,
        timeout: Duration,
    ) -> Result<(), RunError> {
        if !cloud_enabled {
            return Err(RunError::CloudDisabled);
        }
        if self.runtime.provider() != binding.provider {
            return Err(RunError::WrongProvider);
        }
        if timeout.is_zero() || timeout > Duration::from_secs(300) {
            return Err(RunError::Invalid);
        }
        if *cancel.borrow() {
            return Err(RunError::Cancelled);
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn run_claimed(
        &self,
        journal: &mut Journal,
        lease: &Lease,
        binding: &Binding,
        operation: Uuid,
        envelope: &Envelope,
        hash: &str,
        cancel: watch::Receiver<bool>,
        timeout: Duration,
    ) -> Result<Completion, RunError> {
        let recorded = journal.prepare(lease, operation, hash)?;
        if recorded.state == "completed" {
            return Ok(Completion {
                receipt: recorded.receipt.ok_or(RunError::Persistence)?,
                replayed: true,
            });
        }
        if !journal.mark_dispatched(lease, operation)? {
            return Err(RunError::Unknown);
        }
        let work = async {
            let reply = self.runtime.send(binding, operation, envelope).await?;
            Ok(reply)
        };
        let reply = match guarded(journal, lease, cancel, timeout, work).await {
            Ok(reply) => reply,
            Err(error) => {
                let _ = tokio::time::timeout(
                    Duration::from_secs(2),
                    self.runtime.interrupt(binding, operation),
                )
                .await;
                return Err(error);
            }
        };
        self.commit(journal, lease, binding, operation, reply).await
    }
    async fn commit(
        &self,
        journal: &mut Journal,
        lease: &Lease,
        binding: &Binding,
        operation: Uuid,
        reply: ProviderReply,
    ) -> Result<Completion, RunError> {
        if reply.text.is_empty() || reply.text.len() > 256 * 1024 {
            return Err(RunError::Invalid);
        }
        journal.acknowledge(lease, operation, &reply.turn_id)?;
        // Bound result persistence well below the lease period; DB fencing still
        // rejects expiry or a takeover during a slow store call.
        let receipt = tokio::time::timeout(
            Duration::from_secs(5),
            self.results.persist(binding, operation, &reply),
        )
        .await
        .map_err(|_| RunError::Persistence)??;
        journal.finish(lease, operation, &reply.turn_id, &receipt, true)?;
        Ok(Completion {
            receipt,
            replayed: false,
        })
    }
    /// Deliberate recovery operation. Never invokes send, even if no result is found.
    pub async fn reconcile(
        &self,
        journal: &mut Journal,
        binding: &Binding,
        operation: Uuid,
        cloud_enabled: bool,
        cancel: watch::Receiver<bool>,
    ) -> Result<Completion, RunError> {
        self.validate(binding, cloud_enabled, &cancel, Duration::from_secs(60))?;
        let lease = journal.acquire(binding, Uuid::new_v4(), 30)?;
        let result = async {
            let pending = journal
                .pending(&lease)?
                .into_iter()
                .find(|p| p.operation == operation)
                .ok_or(RunError::Unknown)?;
            if pending.record.state != "delivery_unknown" {
                return Err(RunError::Unknown);
            }
            let work = async {
                if let Some(reply) = self.results.load(binding, operation).await? {
                    return Ok(Some(reply));
                }
                self.runtime
                    .reconcile(binding, operation, pending.record.provider_turn.as_deref())
                    .await
            };
            let reply = guarded(journal, &lease, cancel, Duration::from_secs(60), work)
                .await?
                .ok_or(RunError::Unknown)?;
            self.commit(journal, &lease, binding, operation, reply)
                .await
        }
        .await;
        let released = journal.release(&lease);
        match result {
            Ok(done) => {
                released?;
                Ok(done)
            }
            Err(error) => Err(error),
        }
    }
}
/// Keep the writer live during provider I/O, and fail closed if renewal fails.
async fn guarded<T>(
    journal: &mut Journal,
    lease: &Lease,
    mut cancel: watch::Receiver<bool>,
    timeout: Duration,
    work: impl std::future::Future<Output = Result<T, RunError>>,
) -> Result<T, RunError> {
    tokio::pin!(work);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    loop {
        if *cancel.borrow() {
            return Err(RunError::Cancelled);
        }
        tokio::select! {
            biased;
            changed=cancel.changed()=>{if changed.is_err() || *cancel.borrow() {return Err(RunError::Cancelled);}},
            _=&mut deadline=>return Err(RunError::Timeout),
            _=heartbeat.tick()=>journal.renew(lease,30)?,
            reply=&mut work=>return reply,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    };
    struct Runtime {
        calls: Arc<AtomicUsize>,
        lost: bool,
        hang: bool,
        interrupts: Arc<AtomicUsize>,
    }
    fn answer() -> ProviderReply {
        ProviderReply {
            turn_id: "provider-1".into(),
            text: "answer".into(),
        }
    }
    #[async_trait]
    impl SubscriptionRuntime for Runtime {
        fn provider(&self) -> Provider {
            Provider::Copilot
        }
        async fn send(
            &self,
            _: &Binding,
            _: Uuid,
            _: &Envelope,
        ) -> Result<ProviderReply, RunError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.hang {
                std::future::pending::<()>().await;
            }
            if self.lost {
                return Err(RunError::Unknown);
            }
            Ok(answer())
        }
        async fn reconcile(
            &self,
            _: &Binding,
            _: Uuid,
            _: Option<&str>,
        ) -> Result<Option<ProviderReply>, RunError> {
            Ok(Some(answer()))
        }
        async fn interrupt(&self, _: &Binding, _: Uuid) {
            self.interrupts.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[derive(Default)]
    struct Store {
        fail: AtomicBool,
        saved: Mutex<std::collections::HashMap<(Uuid, Uuid), String>>,
    }
    #[async_trait]
    impl ResultStore for Store {
        async fn persist(
            &self,
            binding: &Binding,
            op: Uuid,
            reply: &ProviderReply,
        ) -> Result<String, RunError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(RunError::Persistence);
            }
            let mut map = self.saved.lock().unwrap();
            if let Some(old) = map.get(&(binding.session, op)) {
                if old != &reply.text {
                    return Err(RunError::Persistence);
                }
            }
            map.insert((binding.session, op), reply.text.clone());
            Ok(format!("receipt-{op}"))
        }
    }
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("hive-runner-{}.db", Uuid::new_v4())))
        }
        fn open(&self) -> Journal {
            Journal::open(&self.0).unwrap()
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
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
    fn envelope() -> Envelope {
        Envelope {
            model: "test".into(),
            prompt: "hello".into(),
        }
    }
    fn runner(lost: bool, hang: bool) -> TurnRunner<Runtime, Store> {
        TurnRunner {
            runtime: Runtime {
                calls: Arc::new(AtomicUsize::new(0)),
                lost,
                hang,
                interrupts: Arc::new(AtomicUsize::new(0)),
            },
            results: Store::default(),
        }
    }
    const TIMEOUT: Duration = Duration::from_secs(5);
    #[tokio::test]
    async fn completed_turn_reopens_without_a_second_send() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let r = runner(false, false);
        let (_tx, rx) = watch::channel(false);
        let first = r
            .run(&mut j, &b, op, &envelope(), true, rx.clone(), TIMEOUT)
            .await
            .unwrap();
        assert!(!first.replayed);
        drop(j);
        let mut j = temp.open();
        let second = r
            .run(&mut j, &b, op, &envelope(), true, rx.clone(), TIMEOUT)
            .await
            .unwrap();
        assert!(second.replayed);
        assert_eq!(first.receipt, second.receipt);
        assert_eq!(r.runtime.calls.load(Ordering::SeqCst), 1);
        let mut changed = envelope();
        changed.model = "another".into();
        assert!(r
            .run(&mut j, &b, op, &changed, true, rx, TIMEOUT)
            .await
            .is_err());
        assert_eq!(r.runtime.calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn lost_response_requires_explicit_reconciliation_not_resend() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let r = runner(true, false);
        let (_tx, rx) = watch::channel(false);
        assert!(matches!(
            r.run(&mut j, &b, op, &envelope(), true, rx.clone(), TIMEOUT)
                .await,
            Err(RunError::Unknown)
        ));
        assert!(r
            .run(&mut j, &b, op, &envelope(), true, rx.clone(), TIMEOUT)
            .await
            .is_err());
        assert!(r
            .run(
                &mut j,
                &b,
                Uuid::new_v4(),
                &envelope(),
                true,
                rx.clone(),
                TIMEOUT
            )
            .await
            .is_err());
        r.reconcile(&mut j, &b, op, true, rx.clone()).await.unwrap();
        assert!(
            r.run(&mut j, &b, op, &envelope(), true, rx, TIMEOUT)
                .await
                .unwrap()
                .replayed
        );
        assert_eq!(r.runtime.calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn failed_result_write_preserves_provider_id_for_recovery() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let r = runner(false, false);
        let (_tx, rx) = watch::channel(false);
        r.results.fail.store(true, Ordering::SeqCst);
        assert!(matches!(
            r.run(&mut j, &b, op, &envelope(), true, rx.clone(), TIMEOUT)
                .await,
            Err(RunError::Persistence)
        ));
        let lease = j.acquire(&b, Uuid::new_v4(), 30).unwrap();
        let pending = j.pending(&lease).unwrap();
        assert_eq!(
            pending[0].record.provider_turn.as_deref(),
            Some("provider-1")
        );
        j.release(&lease).unwrap();
        r.results.fail.store(false, Ordering::SeqCst);
        r.reconcile(&mut j, &b, op, true, rx).await.unwrap();
        assert_eq!(r.runtime.calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn timeout_interrupts_but_never_requeues() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let r = runner(false, true);
        let (_tx, rx) = watch::channel(false);
        assert!(matches!(
            r.run(
                &mut j,
                &b,
                op,
                &envelope(),
                true,
                rx.clone(),
                Duration::from_millis(20)
            )
            .await,
            Err(RunError::Timeout)
        ));
        assert!(r
            .run(&mut j, &b, op, &envelope(), true, rx, TIMEOUT)
            .await
            .is_err());
        assert_eq!(r.runtime.calls.load(Ordering::SeqCst), 1);
        assert_eq!(r.runtime.interrupts.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn cancellation_during_send_remains_unknown() {
        let temp = Temp::new();
        let mut j = temp.open();
        let b = binding();
        let op = Uuid::new_v4();
        let r = runner(false, true);
        let (tx, rx) = watch::channel(false);
        let calls = r.runtime.calls.clone();
        let cancel = tokio::spawn(async move {
            while calls.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
            tx.send(true).unwrap();
        });
        assert!(matches!(
            r.run(&mut j, &b, op, &envelope(), true, rx, TIMEOUT).await,
            Err(RunError::Cancelled)
        ));
        cancel.await.unwrap();
        let lease = j.acquire(&b, Uuid::new_v4(), 30).unwrap();
        assert_eq!(
            j.pending(&lease).unwrap()[0].record.state,
            "delivery_unknown"
        );
        assert!(!j.mark_dispatched(&lease, op).unwrap());
        assert_eq!(r.runtime.calls.load(Ordering::SeqCst), 1);
        assert_eq!(r.runtime.interrupts.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn consent_provider_and_precancel_guards_make_no_calls() {
        let temp = Temp::new();
        let mut j = temp.open();
        let mut b = binding();
        let r = runner(false, false);
        let (tx, rx) = watch::channel(false);
        assert!(matches!(
            r.run(
                &mut j,
                &b,
                Uuid::new_v4(),
                &envelope(),
                false,
                rx.clone(),
                TIMEOUT
            )
            .await,
            Err(RunError::CloudDisabled)
        ));
        b.provider = Provider::Chatgpt;
        assert!(matches!(
            r.run(
                &mut j,
                &b,
                Uuid::new_v4(),
                &envelope(),
                true,
                rx.clone(),
                TIMEOUT
            )
            .await,
            Err(RunError::WrongProvider)
        ));
        b.provider = Provider::Copilot;
        tx.send(true).unwrap();
        assert!(matches!(
            r.run(&mut j, &b, Uuid::new_v4(), &envelope(), true, rx, TIMEOUT)
                .await,
            Err(RunError::Cancelled)
        ));
        assert_eq!(r.runtime.calls.load(Ordering::SeqCst), 0);
    }
}
