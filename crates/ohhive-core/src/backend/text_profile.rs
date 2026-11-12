//! Opt-in local inference profile. Credentials stay in a private owner file, never in RPC.
use super::llama_cpp::LlamaCppBackend;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TextBackendProfile {
    #[serde(default)]
    pub kind: TextBackendKind,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub credential_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_coding_think: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_checker_think: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_owner_file: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextBackendKind {
    #[default]
    Legacy,
    Tensorfold,
}
impl TextBackendProfile {
    fn path() -> PathBuf {
        crate::nodeconfig::path().with_file_name("text-backend.json")
    }
    pub fn load() -> Result<Self> {
        let mut profile: Self = match std::fs::read(Self::path()) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("Invalid local text backend profile")?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(_) => bail!("Cannot read local text backend profile"),
        };
        let policy_path =
            crate::nodeconfig::path().with_file_name("inference-workflow-policy.json");
        profile.apply_machine_policy(&policy_path)?;
        Ok(profile)
    }
    // Machine admission survives every backend choice, including restoring Ollama.
    fn apply_machine_policy(&mut self, policy_path: &std::path::Path) -> Result<()> {
        match std::fs::symlink_metadata(policy_path) {
            Ok(_) => {
                let text =
                    read_private_text(policy_path.to_str().context("Invalid policy path")?, 4096)?;
                let policy: serde_json::Value =
                    serde_json::from_str(&text).context("Invalid machine workflow policy")?;
                if policy["schema_version"].as_u64() != Some(1) {
                    bail!("Invalid machine workflow policy version");
                }
                let path = policy["owner_file"]
                    .as_str()
                    .filter(|p| std::path::Path::new(p).is_absolute())
                    .context("Invalid machine workflow owner path")?;
                self.workflow_owner_file = Some(path.into());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("Cannot read machine workflow policy"),
        }
        Ok(())
    }
    pub fn backend(&self, legacy_url: &str) -> Result<LlamaCppBackend> {
        match self.kind {
            TextBackendKind::Legacy => {
                let mut backend = LlamaCppBackend::new(legacy_url);
                backend.set_profile_policy(None, self.workflow_owner_file.clone());
                Ok(backend)
            }
            TextBackendKind::Tensorfold => {
                if self.thinking_budget.is_some_and(|v| v == 0 || v > 4096) {
                    bail!("Thinking budget must be between 1 and 4096");
                }
                if self.default_model.as_ref().is_some_and(|v| {
                    v.is_empty() || v.len() > 256 || v.chars().any(char::is_whitespace)
                }) {
                    bail!("Invalid default model");
                }
                if self
                    .workflow_owner_file
                    .as_ref()
                    .is_some_and(|v| !std::path::Path::new(v).is_absolute())
                {
                    bail!("Workflow ownership requires an absolute private state path");
                }
                let mut backend = LlamaCppBackend::tensorfold(&self.origin, &self.credential_file)?;
                backend.set_profile_policy(self.thinking_budget, self.workflow_owner_file.clone());
                Ok(backend)
            }
        }
    }
    pub fn save(&self, legacy_url: &str) -> Result<()> {
        self.backend(legacy_url)?; // Fail closed before replacing the previous selection.
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> Result<()> {
            use std::io::Write;
            let mut file = options.open(&temp)?;
            file.write_all(&serde_json::to_vec(self)?)?;
            file.sync_all()?;
            std::fs::rename(&temp, &path)?;
            std::fs::File::open(path.parent().unwrap())?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temp);
        }
        result
    }
}
/// An immutable reservation binding; a later Den reservation cannot revive an old worker.
#[derive(Clone, Debug)]
pub struct WorkflowLease {
    path: String,
    generation: uuid::Uuid,
}
impl WorkflowLease {
    pub fn capture(path: &str) -> Result<Self> {
        let text = read_private_text(path, 16384)?;
        let state: serde_json::Value =
            serde_json::from_str(&text).context("Invalid workflow ownership state")?;
        if state["schema_version"].as_u64() != Some(1) || state["owner"].as_str() != Some("den") {
            bail!("Reserve Helheim for Den before starting agent work");
        }
        let generation = state["generation"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .context("Invalid workflow ownership generation")?;
        let lease = Self {
            path: path.into(),
            generation,
        };
        lease.admission_open()?;
        Ok(lease)
    }
    pub fn generation(&self) -> String {
        self.generation.to_string()
    }
    fn fence_path(path: &str) -> Result<PathBuf> {
        Ok(std::path::Path::new(path)
            .parent()
            .context("Invalid ownership path")?
            .join("den-drained.json"))
    }
    fn admission_open(&self) -> Result<()> {
        let fence = Self::fence_path(&self.path)?;
        match std::fs::symlink_metadata(&fence) {
            Ok(_) => {
                let v: serde_json::Value = serde_json::from_str(&read_private_text(
                    fence.to_str().context("Invalid fence path")?,
                    4096,
                )?)?;
                let generation = v["generation"]
                    .as_str()
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .context("Invalid admission fence")?;
                if v["schema_version"].as_u64() != Some(1) || generation == self.generation {
                    bail!("Den admission is closed; acquire a new workflow reservation");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => bail!("Cannot verify Den admission fence"),
        }
        Ok(())
    }
    /// Close new admissions persistently before waiting for whole-workflow cleanup.
    pub fn close_current(path: &str) -> Result<serde_json::Value> {
        let state: serde_json::Value = serde_json::from_str(&read_private_text(path, 16384)?)?;
        let generation = state["generation"]
            .as_str()
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .context("Invalid ownership generation")?;
        if state["schema_version"].as_u64() != Some(1)
            || !matches!(
                state["owner"].as_str(),
                Some("hermione" | "transition" | "den" | "fault")
            )
        {
            bail!("Invalid workflow ownership state");
        }
        if state["owner"].as_str() == Some("den") {
            use std::io::Write;
            let path = Self::fence_path(path)?;
            let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(&serde_json::to_vec(
                &serde_json::json!({"schema_version":1,"generation":generation}),
            )?)?;
            file.sync_all()?;
            std::fs::rename(&temp, &path)?;
            std::fs::File::open(path.parent().unwrap())?.sync_all()?;
        }
        Ok(serde_json::json!({"owner":state["owner"],"generation":generation}))
    }
    pub fn check(&self) -> Result<()> {
        let current = Self::capture(&self.path)?;
        if current.generation != self.generation {
            bail!("Workflow ownership changed; restart the Den worker after a new reservation");
        }
        Ok(())
    }
}
impl TextBackendProfile {
    pub fn workflow_lease(&self) -> Result<Option<WorkflowLease>> {
        self.workflow_owner_file
            .as_deref()
            .map(WorkflowLease::capture)
            .transpose()
    }
}
fn read_private_text(path: &str, limit: u64) -> Result<String> {
    if !std::path::Path::new(path).is_absolute() {
        bail!("Use an absolute private file path");
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| anyhow::anyhow!("Cannot open private backend file"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        bail!("Invalid private backend file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::getuid() } || metadata.mode() & 0o077 != 0 {
            bail!("Backend file must be private and owned by this user");
        }
    }
    let mut text = String::new();
    file.take(limit + 1).read_to_string(&mut text)?;
    if text.len() as u64 > limit {
        bail!("Private backend file exceeds bound");
    }
    Ok(text)
}
pub(super) fn read_credential(path: &str) -> Result<String> {
    if !std::path::Path::new(path).is_absolute() {
        bail!("Use an absolute private credential file path");
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| anyhow::anyhow!("Cannot open TensorFold credential file"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 4096 {
        bail!("Invalid TensorFold credential file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::getuid() } || metadata.mode() & 0o077 != 0 {
            bail!("TensorFold credential file must be owned by this user and private (0600)");
        }
    }
    let mut value = String::new();
    file.take(4097).read_to_string(&mut value)?;
    let token = value.trim();
    if token.is_empty() || token.len() > 4096 || token.bytes().any(|b| b <= 32 || b >= 127) {
        bail!("Invalid TensorFold credential");
    }
    Ok(token.into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    fn key() -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("den-tensorfold-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("key");
        std::fs::write(&path, "fixture-secret").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        (dir, path)
    }
    #[test]
    fn credential_rejects_public_symlink_and_invalid_data() {
        let (dir, path) = key();
        assert_eq!(
            read_credential(path.to_str().unwrap()).unwrap(),
            "fixture-secret"
        );
        let link = dir.join("link");
        symlink(&path, &link).unwrap();
        assert!(read_credential(link.to_str().unwrap()).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_credential(path.to_str().unwrap()).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, "secret\ninjected").unwrap();
        assert!(read_credential(path.to_str().unwrap()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn tensorfold_rejects_nonloopback_credentials_in_url_and_redirect_targets() {
        for origin in [
            "https://example.com",
            "http://user:secret@127.0.0.1:18090",
            "http://127.0.0.1:18090/v1",
            "http://127.0.0.1:18090?key=secret",
        ] {
            assert!(LlamaCppBackend::tensorfold(origin, "/does/not/exist").is_err());
        }
        assert!(serde_json::from_str::<TextBackendProfile>(r#"{"kind":"typo"}"#).is_err());
        assert!(serde_json::from_str::<TextBackendProfile>(
            r#"{"kind":"tensorfold","token":"secret"}"#
        )
        .is_err());
    }
    #[test]
    fn workflow_lease_rejects_peer_transition_stale_and_insecure_state() {
        let (dir, path) = key();
        let write = |owner: &str, generation: uuid::Uuid| {
            std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"schema_version":1,"owner":owner,"generation":generation,"diagnostic":"ignored"})).unwrap()).unwrap()
        };
        let generation = uuid::Uuid::new_v4();
        for owner in ["hermione", "transition", "fault"] {
            write(owner, generation);
            assert!(WorkflowLease::capture(path.to_str().unwrap()).is_err());
        }
        write("den", generation);
        let lease = WorkflowLease::capture(path.to_str().unwrap()).unwrap();
        lease.check().unwrap();
        WorkflowLease::close_current(path.to_str().unwrap()).unwrap();
        assert!(lease.check().is_err());
        assert!(WorkflowLease::capture(path.to_str().unwrap()).is_err());
        write("den", uuid::Uuid::new_v4());
        assert!(WorkflowLease::capture(path.to_str().unwrap()).is_ok());
        assert!(lease.check().is_err());
        write("den", generation);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(lease.check().is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(lease.check().is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn legacy_profile_deserializes_without_migration_or_silent_default_change() {
        let profile: TextBackendProfile =
            serde_json::from_str(r#"{"kind":"legacy","origin":"","credential_file":""}"#).unwrap();
        assert_eq!(profile.kind, TextBackendKind::Legacy);
        assert!(profile.default_model.is_none());
        assert!(profile.workflow_owner_file.is_none());
    }

    #[tokio::test]
    async fn machine_policy_keeps_legacy_inference_gated() {
        let (dir, path) = key();
        let owner = dir.join("owner.json");
        std::fs::write(&owner,serde_json::to_vec(&serde_json::json!({"schema_version":1,"owner":"hermione","generation":uuid::Uuid::new_v4()})).unwrap()).unwrap();
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"schema_version":1,"owner_file":owner}))
                .unwrap(),
        )
        .unwrap();
        let mut profile = TextBackendProfile::default();
        profile.apply_machine_policy(&path).unwrap();
        assert_eq!(profile.kind, TextBackendKind::Legacy);
        assert!(profile.workflow_owner_file.is_some());
        let backend = profile.backend("http://127.0.0.1:1").unwrap();
        assert!(matches!(
            backend.chat_with_tools("fixture", &[], &[], 64).await,
            Err(crate::backend::BackendError::Rejected(_))
        ));
        std::fs::write(&path, r#"{"schema_version":2}"#).unwrap();
        assert!(profile.apply_machine_policy(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn authenticated_discovery_tool_turn_and_metadata_never_use_ollama_endpoints() {
        use crate::backend::Backend;
        use axum::{
            http::HeaderMap,
            routing::{get, post},
            Json, Router,
        };
        let (dir, path) = key();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls = hits.clone();
        let auth = |headers: &HeaderMap| {
            assert_eq!(
                headers.get("authorization").unwrap(),
                "Bearer fixture-secret"
            )
        };
        let app=Router::new().route("/v1/models",get(move |headers:HeaderMap| async move {
            auth(&headers);Json(serde_json::json!({"data":[{"id":"fixture","context_length":8192}]}))
        })).route("/v1/chat/completions",post(move |headers:HeaderMap, Json(body):Json<serde_json::Value>| {let calls=calls.clone(); async move {
            auth(&headers);assert_eq!(body["model"],"fixture");assert_eq!(body["thinking_budget"],1024);calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            Json(serde_json::json!({"model":"fixture","choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1}}))
        }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut backend =
            LlamaCppBackend::tensorfold(&format!("http://{addr}"), path.to_str().unwrap()).unwrap();
        backend.set_profile_policy(Some(1024), None);
        let caps = backend.capabilities().await.unwrap();
        assert_eq!(caps.models[0].backend, "tensorfold");
        assert_eq!(
            backend.model_tool_capabilities("fixture").await.unwrap(),
            (None, None)
        );
        assert_eq!(
            backend
                .model_runtime_info("fixture")
                .await
                .unwrap()
                .maximum_context,
            Some(8192)
        );
        // The coding protocol is the same tool-chat transport used by Den's existing runner.
        let owner = dir.join("owner.json");
        std::fs::write(&owner,serde_json::to_vec(&serde_json::json!({"schema_version":1,"owner":"hermione","generation":uuid::Uuid::new_v4()})).unwrap()).unwrap();
        std::fs::set_permissions(&owner, std::fs::Permissions::from_mode(0o600)).unwrap();
        backend.set_profile_policy(Some(1024), Some(owner.to_str().unwrap().into()));
        assert!(backend
            .chat_with_tools("fixture", &[], &[], 64)
            .await
            .is_err());
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        std::fs::write(&owner,serde_json::to_vec(&serde_json::json!({"schema_version":1,"owner":"den","generation":uuid::Uuid::new_v4()})).unwrap()).unwrap();
        backend.set_profile_policy(Some(1024), Some(owner.to_str().unwrap().into()));
        let result = backend
            .chat_with_tools("fixture", &[], &[], 64)
            .await
            .unwrap();
        assert!(
            matches!(result.0, super::super::llama_cpp::ToolChatResult::Text(ref text) if text == "done")
        );
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        WorkflowLease::close_current(owner.to_str().unwrap()).unwrap();
        assert!(backend
            .chat_with_tools("fixture", &[], &[], 64)
            .await
            .is_err());
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
