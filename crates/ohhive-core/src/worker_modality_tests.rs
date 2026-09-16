//! Regression: unsupported cards cannot infer, complete, checkpoint, or release/requeue.
#![allow(unused_variables)]
use super::*;
use crate::backend::{BackendError, ChunkStream};
use crate::hub::{Completion, HubError, McpServerConfig, SpawnedCard};
use std::sync::Mutex;
#[derive(Default)]
struct RefusalHub {
    failures: Mutex<Vec<(Uuid, String)>>,
}
#[async_trait::async_trait]
impl Hub for RefusalHub {
    async fn claim_card(&self) -> Result<Claim, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn complete_card(
        &self,
        card_id: Uuid,
        content: &str,
        model_id: Option<&str>,
        usage: crate::ledger::Usage,
    ) -> Result<Completion, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn checkpoint(
        &self,
        card_id: Uuid,
        step: u32,
        state: &serde_json::Value,
        usage: crate::ledger::Usage,
    ) -> Result<serde_json::Value, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn fail_card(&self, card_id: Uuid, reason: &str) -> Result<serde_json::Value, HubError> {
        self.failures.lock().unwrap().push((card_id, reason.into()));
        Ok(serde_json::json!({}))
    }
    async fn release_card(
        &self,
        card_id: Uuid,
        reason: &str,
    ) -> Result<serde_json::Value, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn spawn_child_card(
        &self,
        parent_card_id: Uuid,
        key: &str,
        title: &str,
        modality: &str,
        inputs: &str,
        acceptance: &str,
        required_capabilities: serde_json::Value,
    ) -> Result<SpawnedCard, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn wait_on_child(
        &self,
        card_id: Uuid,
        child_card_id: Uuid,
    ) -> Result<serde_json::Value, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn mcp_server_config(&self, server_id: Uuid) -> Result<McpServerConfig, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn check_in(
        &self,
        caps: &Capabilities,
        region: Option<&str>,
    ) -> Result<serde_json::Value, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn heartbeat(&self, prev_rtt_ms: Option<u64>) -> Result<(String, u64), HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn check_out(&self) -> Result<String, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn get_schedule(&self) -> Result<Option<serde_json::Value>, HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
    async fn post_activity(
        &self,
        event_type: &str,
        body: &str,
        payload: serde_json::Value,
    ) -> Result<(), HubError> {
        panic!("unexpected hub action for unsupported modality")
    }
}
struct NeverInfer;
#[async_trait::async_trait]
impl Backend for NeverInfer {
    fn name(&self) -> &'static str {
        "never"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    async fn capabilities(&self) -> std::result::Result<Capabilities, BackendError> {
        panic!("unexpected probe")
    }
    async fn run<'a>(&'a self, job: &'a Job) -> std::result::Result<ChunkStream<'a>, BackendError> {
        panic!("unsupported modality reached inference")
    }
}
#[tokio::test]
async fn unsupported_jobs_fail_once_without_inference_or_payment() {
    let hub = RefusalHub::default();
    let backend = NeverInfer;
    let caps = crate::backend::mock::MockBackend
        .capabilities()
        .await
        .unwrap();
    let (_stop, receiver) = watch::channel(false);
    let (events, mut rx) = broadcast::channel(16);
    let worker = Worker {
        capacity_path: std::env::temp_dir().join("hive-modality-unused"),
        hub: &hub,
        backend: &backend,
        caps: &caps,
        default_model: None,
        stop: receiver,
        events: Some(events),
        #[cfg(feature = "sandbox")]
        data_dir: std::env::temp_dir(),
        #[cfg(feature = "sandbox")]
        sandbox: None,
    };
    for modality in ["image", "video", "music", "unknown", "", "Text"] {
        let id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let card = ClaimedCard {
            id,
            project_id,
            key: "unsupported".into(),
            title: "Unsupported".into(),
            modality: modality.into(),
            inputs: "Never infer".into(),
            acceptance: String::new(),
            deps: vec![],
            requires_internet: false,
            required_capabilities: serde_json::json!({}),
        };
        let project = ClaimedProject {
            id: project_id,
            title: "Project".into(),
            goal: String::new(),
        };
        worker
            .run_card(
                card,
                project,
                serde_json::Map::new(),
                None,
                chrono::Utc::now() + chrono::Duration::minutes(1),
            )
            .await
            .unwrap();
        assert!(matches!(rx.try_recv().unwrap(), WorkerEvent::Failed { .. }));
        let failures = hub.failures.lock().unwrap();
        assert_eq!(failures.last().unwrap().0, id);
        assert!(failures.last().unwrap().1.contains("unsupported_modality"));
    }
    assert_eq!(hub.failures.lock().unwrap().len(), 6);
    assert!(rx.try_recv().is_err());
}
