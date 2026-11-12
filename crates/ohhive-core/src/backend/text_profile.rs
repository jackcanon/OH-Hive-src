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
        match std::fs::read(Self::path()) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).context("Invalid local text backend profile")
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => bail!("Cannot read local text backend profile"),
        }
    }
    pub fn backend(&self, legacy_url: &str) -> Result<LlamaCppBackend> {
        match self.kind {
            TextBackendKind::Legacy => Ok(LlamaCppBackend::new(legacy_url)),
            TextBackendKind::Tensorfold => {
                LlamaCppBackend::tensorfold(&self.origin, &self.credential_file)
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
            auth(&headers);assert_eq!(body["model"],"fixture");calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
            Json(serde_json::json!({"model":"fixture","choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":1}}))
        }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let backend =
            LlamaCppBackend::tensorfold(&format!("http://{addr}"), path.to_str().unwrap()).unwrap();
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
        let result = backend
            .chat_with_tools("fixture", &[], &[], 64)
            .await
            .unwrap();
        assert!(
            matches!(result.0, super::super::llama_cpp::ToolChatResult::Text(ref text) if text == "done")
        );
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        server.abort();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
