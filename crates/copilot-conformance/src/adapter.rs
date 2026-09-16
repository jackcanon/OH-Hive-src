//! Opt-in concrete SDK adapter for Hive's fenced runner. Not registered with Bots yet.
//! Trusted host supplies an existing private directory and selected-account credentials.
//! One isolated SDK session per operation: the runner supplies all authorized context.
use async_trait::async_trait;
use github_copilot_sdk::{session::Session, Client};
use hive_core::subscription::{
    journal::{Binding, Provider},
    runner::{Envelope, ProviderReply, RunError, SubscriptionRuntime},
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct CopilotRuntime {
    client: Client,
    binding: Binding,
    home: PathBuf,
    active: Mutex<Option<(Uuid, Arc<Session>)>>,
    sending: Mutex<()>,
}

impl CopilotRuntime {
    /// Performs account verification, never a model request. Caller must stop on
    /// sign-out/account changes and discard this instance; credentials aren't refreshed here.
    pub async fn start(
        binding: Binding,
        token: String,
        login: &str,
        home: &Path,
        cloud_enabled: bool,
    ) -> Result<Self, RunError> {
        if !cloud_enabled {
            return Err(RunError::CloudDisabled);
        }
        if binding.provider != Provider::Copilot {
            return Err(RunError::WrongProvider);
        }
        if login.is_empty() || login.len() > 256 {
            return Err(RunError::Invalid);
        }
        if binding.policy_revision.is_empty() || binding.policy_revision.len() > 256 {
            return Err(RunError::Invalid);
        }
        bind_home(home, &binding)?;
        let mut options = crate::options(token.clone(), home).map_err(|_| RunError::Invalid)?;
        // Never let COPILOT_CLI_PATH or a developer's cached runtime override
        // the pinned runtime shipped by this host.
        options.program = github_copilot_sdk::CliProgram::Path(
            github_copilot_sdk::install_bundled_runtime().ok_or(RunError::Invalid)?,
        );
        // Verify the exact credential, never the ambient CLI/Keychain identity.
        #[derive(Deserialize)]
        struct Identity {
            login: String,
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| RunError::Unknown)?;
        let identity: Identity = http
            .get("https://api.github.com/user")
            .bearer_auth(token)
            .header("User-Agent", "Lokis-Den-Copilot")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|_| RunError::Unknown)?
            .error_for_status()
            .map_err(|_| RunError::Unknown)?
            .json()
            .await
            .map_err(|_| RunError::Unknown)?;
        if !identity.login.eq_ignore_ascii_case(login) {
            return Err(RunError::Invalid);
        }
        let client = tokio::time::timeout(Duration::from_secs(30), Client::start(options))
            .await
            .map_err(|_| RunError::Timeout)?
            .map_err(|_| RunError::Unknown)?;
        let checked = tokio::time::timeout(Duration::from_secs(15), client.get_auth_status()).await;
        let valid = match checked {
            Ok(Ok(auth)) => crate::verified_token_identity_error(
                auth.is_authenticated,
                auth.login.as_deref(),
                &identity.login,
                login,
            )
            .is_none(),
            _ => false,
        };
        if !valid {
            let _ = tokio::time::timeout(Duration::from_secs(5), client.stop()).await;
            return Err(RunError::Unknown);
        }
        Ok(Self {
            client,
            binding,
            home: home.into(),
            active: Mutex::new(None),
            sending: Mutex::new(()),
        })
    }
    pub async fn stop(&self) {
        let _ = tokio::time::timeout(Duration::from_secs(5), self.client.stop()).await;
    }
    fn check(&self, binding: &Binding) -> Result<(), RunError> {
        if binding != &self.binding {
            return Err(RunError::Invalid);
        }
        Ok(())
    }
}

fn session_id(binding: &Binding, operation: Uuid) -> String {
    format!("hive-{}-{}", binding.session, operation)
}

// Bind the runtime's persisted state to the exact trusted account/conversation.
// The caller creates a separate private directory for each journal session.
fn bind_home(home: &Path, binding: &Binding) -> Result<(), RunError> {
    if !home.is_absolute() {
        return Err(RunError::Invalid);
    }
    let meta = std::fs::symlink_metadata(home).map_err(|_| RunError::Persistence)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(RunError::Invalid);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err(RunError::Invalid);
        }
    }
    let path = home.join("hive-binding.json");
    let encoded = serde_json::to_vec(binding).map_err(|_| RunError::Invalid)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(&encoded)
                .and_then(|_| file.sync_all())
                .map_err(|_| RunError::Persistence)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let meta = std::fs::symlink_metadata(&path).map_err(|_| RunError::Persistence)?;
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 8192 {
                return Err(RunError::Invalid);
            }
            if std::fs::read(path).map_err(|_| RunError::Persistence)? != encoded {
                return Err(RunError::Invalid);
            }
        }
        Err(_) => return Err(RunError::Persistence),
    }
    Ok(())
}

#[async_trait]
impl SubscriptionRuntime for CopilotRuntime {
    fn provider(&self) -> Provider {
        Provider::Copilot
    }
    async fn send(
        &self,
        binding: &Binding,
        operation: Uuid,
        envelope: &Envelope,
    ) -> Result<ProviderReply, RunError> {
        self.check(binding)?;
        let _guard = self.sending.try_lock().map_err(|_| RunError::Unknown)?;
        // A dropped earlier call stays registered until interrupted/stopped.
        if self.active.lock().await.is_some() {
            return Err(RunError::Unknown);
        }
        if envelope.model.is_empty()
            || envelope.model.len() > 256
            || envelope.prompt.is_empty()
            || envelope.prompt.len() > 128 * 1024
        {
            return Err(RunError::Invalid);
        }
        let models = self
            .client
            .list_models()
            .await
            .map_err(|_| RunError::Unknown)?;
        if !models.iter().any(|model| model.id == envelope.model) {
            return Err(RunError::Invalid);
        }
        let id = session_id(binding, operation);
        let session = Arc::new(
            self.client
                .create_session(crate::session(&id, &envelope.model, &self.home))
                .await
                .map_err(|_| RunError::Unknown)?,
        );
        *self.active.lock().await = Some((operation, session.clone()));
        // SDK registers its terminal-event waiter BEFORE sending. Never retry here.
        let event = session
            .send_and_wait(envelope.prompt.as_str())
            .await
            .map_err(|_| RunError::Unknown)?;
        let event = event.ok_or(RunError::Unknown)?;
        let text = event
            .data
            .get("content")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 256 * 1024)
            .ok_or(RunError::Unknown)?
            .to_owned();
        // Correlation uses the dedicated one-operation session, not a transient event ID.
        let _ = tokio::time::timeout(Duration::from_secs(2), session.disconnect()).await;
        *self.active.lock().await = None;
        Ok(ProviderReply { turn_id: id, text })
    }
    async fn reconcile(
        &self,
        binding: &Binding,
        _operation: Uuid,
        _provider_turn: Option<&str>,
    ) -> Result<Option<ProviderReply>, RunError> {
        self.check(binding)?;
        // Persisted text alone isn't terminal proof. Until the runtime's durable
        // completion evidence is validated, leave missing results unknown.
        // TurnRunner already recovers locally saved replies before reaching here.
        Ok(None)
    }
    async fn interrupt(&self, binding: &Binding, operation: Uuid) {
        if self.check(binding).is_err() {
            return;
        }
        let active = self.active.lock().await.take();
        if let Some((id, session)) = active {
            if id == operation {
                let _ = tokio::time::timeout(Duration::from_millis(800), session.abort()).await;
                let _ =
                    tokio::time::timeout(Duration::from_millis(800), session.disconnect()).await;
            } else {
                *self.active.lock().await = Some((id, session));
            }
        } else {
            // Session creation may have completed remotely before cancellation.
            let _ = tokio::time::timeout(Duration::from_millis(1600), self.client.stop()).await;
        }
    }
}

#[cfg(test)]
mod tests;
