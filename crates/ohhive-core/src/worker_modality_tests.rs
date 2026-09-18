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

#[cfg(all(feature = "sandbox", feature = "llama-cpp"))]
#[tokio::test]
async fn coding_preflight_checks_selected_model_before_workspace_or_inference() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    // Explicit choice wins over node default, which wins over advertised fallback.
    for (explicit, default, status, body, expected_error) in [
        (
            Some("explicit"),
            Some("default"),
            "200 OK",
            r#"{"capabilities":["completion"]}"#,
            Some("does not support coding tools"),
        ),
        (
            None,
            Some("default"),
            "503 Unavailable",
            r#"{}"#,
            Some("Cannot check coding tool support"),
        ),
        (None, None, "200 OK", r#"{"capabilities":["tools"]}"#, None),
        (None, None, "404 Not Found", r#"{}"#, None),
    ] {
        let selected = explicit.or(default).unwrap_or("fallback");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut part = [0; 4096];
                let n = socket.read(&mut part).await.unwrap();
                assert!(n > 0);
                request.extend_from_slice(&part[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some((headers, payload)) = text.split_once("\r\n\r\n") {
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    if payload.len() >= length {
                        assert!(headers.starts_with("POST /api/show "));
                        let json: serde_json::Value = serde_json::from_str(payload).unwrap();
                        assert_eq!(json["model"], selected);
                        break;
                    }
                }
            }
            socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        let hub = RefusalHub::default();
        let backend =
            crate::backend::llama_cpp::LlamaCppBackend::local_only(&format!("http://{addr}"))
                .unwrap();
        let mut caps = crate::backend::mock::MockBackend
            .capabilities()
            .await
            .unwrap();
        caps.tools_level = crate::capability::ToolsLevel::SandboxedTools;
        caps.models[0].id = "fallback".into();
        let dir = std::env::temp_dir().join(format!("hive-preflight-{}", Uuid::new_v4()));
        let data = dir.join("must-not-create");
        let (_stop, receiver) = watch::channel(false);
        let worker = Worker {
            capacity_path: dir.join("capacity"),
            hub: &hub,
            backend: &backend,
            caps: &caps,
            default_model: default.map(str::to_owned),
            stop: receiver,
            events: None,
            data_dir: data.clone(),
            sandbox: None,
        };
        let card = ClaimedCard {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key: "probe".into(),
            title: "Probe".into(),
            modality: "code".into(),
            inputs: "Must not execute".into(),
            acceptance: String::new(),
            deps: vec![],
            requires_internet: false,
            required_capabilities: serde_json::json!({"brain":"local", "task":"Do not run", "model_id":explicit, "workspace_path":data.to_str().unwrap()}),
        };
        if let Some(expected) = expected_error {
            let project = ClaimedProject {
                id: card.project_id,
                title: "Probe".into(),
                goal: String::new(),
            };
            worker
                .run_code_card(
                    card.clone(),
                    project,
                    serde_json::Map::new(),
                    chrono::Utc::now() + chrono::Duration::minutes(1),
                )
                .await
                .unwrap();
            let failures = hub.failures.lock().unwrap();
            assert_eq!(failures.len(), 1);
            assert!(failures[0].1.contains(expected), "{}", failures[0].1);
            assert!(failures[0].1.contains(selected));
        } else {
            let spec = crate::coder::CodeSessionSpec::from_required_capabilities(
                &card.required_capabilities,
            )
            .unwrap();
            assert!(worker.local_brain(&card, &spec).await.is_ok());
        }
        assert!(!data.exists(), "preflight must not prepare a workspace");
        server.await.unwrap();
    }
}
