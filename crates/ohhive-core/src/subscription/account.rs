//! Managed ChatGPT account connection only. No model turns or Hive dispatch.
use super::transport::{classify_frame, FrameReader, FrameWriter, Incoming};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
    time::{timeout, Instant},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountStatus {
    pub state: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub auth_url: Option<String>,
    pub user_code: Option<String>,
    pub detail: String,
}

struct Process {
    child: Child,
    reader: FrameReader<ChildStdout>,
    writer: FrameWriter<ChildStdin>,
    next_id: u64,
    login_completion: Option<Value>,
    stderr_task: tokio::task::JoinHandle<()>,
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.stderr_task.abort();
    }
}
impl Process {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let result = timeout(Duration::from_secs(15), async {
            self.writer.write_value(&json!({"id":id,"method":method,"params":params})).await.map_err(|_| "Cannot write to Codex")?;
            loop {
                let frame = self.reader.read_frame().await.map_err(|_| "Invalid Codex response")?.ok_or("Codex stopped")?;
                match classify_frame(&frame).map_err(|_| "Unsupported Codex response")? {
                    Incoming::Response(reply) if reply.id == id => {
                        // Do not reflect unredacted provider diagnostics/auth payloads into UI logs.
                        return reply.into_result().map(|v| v.unwrap_or(Value::Null)).map_err(|_| "Codex rejected the account request");
                    }
                    Incoming::Notification(event) if event.method == "account/login/completed" => {
                        self.login_completion = event.params;
                    }
                    Incoming::ServerRequest(req) => {
                        self.writer.write_value(&json!({"id": req.id,"error":{"code":-32601,"message":"Account connection does not execute tools"}})).await.map_err(|_| "Cannot decline runtime request")?;
                    }
                    _ => {}
                }
            }
        }).await;
        result
            .map_err(|_| "Codex account request timed out".to_string())?
            .map_err(str::to_string)
    }
}

pub struct AccountConnection {
    process: Option<Process>,
    status: AccountStatus,
    pending: Option<(String, Instant)>,
}
impl Default for AccountConnection {
    fn default() -> Self {
        Self {
            process: None,
            status: AccountStatus {
                state: "not_started".into(),
                detail: "Connect your ChatGPT account to Hive.".into(),
                ..Default::default()
            },
            pending: None,
        }
    }
}

pub static ACCOUNT_CONNECTION: Mutex<Option<AccountConnection>> = Mutex::const_new(None);

pub fn discover_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    let home = dirs::home_dir()?;
    let mut candidates = vec![
        home.join(".local/bin").join(name),
        home.join(".cargo/bin").join(name),
        PathBuf::from("/opt/homebrew/bin").join(name),
        PathBuf::from("/usr/local/bin").join(name),
    ];
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths).map(|p| p.join(name)));
    }
    candidates
        .into_iter()
        .find(|p| p.is_absolute() && p.is_file())
}

fn private_directory(path: &Path) -> Result<(), String> {
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err("The Hive account directory must not be a symlink".into());
    }
    std::fs::create_dir_all(path)
        .map_err(|_| "Cannot create the private Hive account directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot protect the Hive account directory")?;
    }
    Ok(())
}

fn runtime_command(binary: &Path, home: &Path) -> Command {
    let mut command = Command::new(binary);
    // Start with no API keys, proxies, inherited provider settings or arbitrary helper config.
    command.env_clear();
    for key in [
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "WINDIR",
        "PATH",
        "TMPDIR",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
        "DBUS_SESSION_BUS_ADDRESS",
        "XDG_RUNTIME_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env("CODEX_HOME", home);
    command.current_dir(home.join("workspace"));
    command.kill_on_drop(true);
    command
}

fn account_status(account: &Value) -> Result<AccountStatus, String> {
    if account.is_null() {
        return Ok(AccountStatus {
            state: "signed_out".into(),
            detail: "Not connected to ChatGPT.".into(),
            ..Default::default()
        });
    }
    if account.get("type").and_then(Value::as_str) != Some("chatgpt") {
        return Err(
            "This connection requires ChatGPT sign-in; API authentication is not accepted.".into(),
        );
    }
    Ok(AccountStatus { state: "connected".into(), email: account.get("email").and_then(Value::as_str).map(str::to_owned), plan: account.get("planType").and_then(Value::as_str).map(str::to_owned), detail: "ChatGPT account connected. Agent chat and fleet delegation are not enabled by this account screen.".into(), ..Default::default() })
}
fn allowed_auth_url(url: &str) -> bool {
    // Authority must end at a slash; rejects userinfo, suffix hosts, ports and javascript URLs.
    ["https://auth.openai.com/", "https://chatgpt.com/"]
        .iter()
        .any(|prefix| url.starts_with(prefix))
}

impl AccountConnection {
    pub async fn start(&mut self, binary: &Path, home: &Path) -> Result<(), String> {
        self.start_with_version_timeout(binary, home, Duration::from_secs(5)).await
    }

    async fn start_with_version_timeout(
        &mut self, binary: &Path, home: &Path, version_timeout: Duration,
    ) -> Result<(), String> {
        if !binary.is_absolute() || !binary.is_file() {
            return Err("Select an installed Codex executable using its full path.".into());
        }
        private_directory(home)?;
        private_directory(&home.join("workspace"))?;
        // Dedicated managed config only: never read/edit the user's separate Codex home.
        let config = home.join("config.toml");
        if config
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err("Unsafe account configuration path".into());
        }
        std::fs::write(
            config,
            "forced_login_method = \"chatgpt\"\ncli_auth_credentials_store = \"keyring\"\n",
        )
        .map_err(|_| "Cannot write Hive account configuration")?;
        let version = timeout(
            version_timeout,
            runtime_command(binary, home).arg("--version").output(),
        )
        .await
        .map_err(|_| "Codex version check timed out")?
        .map_err(|_| "Cannot launch Codex")?;
        if !version.status.success()
            || String::from_utf8_lossy(&version.stdout).trim() != "codex-cli 0.149.0"
        {
            return Err(
                "This preview requires Codex 0.149.0, the verified runtime version.".into(),
            );
        }
        let mut child = runtime_command(binary, home)
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "Cannot start Codex account service")?;
        let reader = FrameReader::new(
            child.stdout.take().ok_or("Missing Codex stdout")?,
            8 * 1024 * 1024,
        );
        let writer = FrameWriter::new(child.stdin.take().ok_or("Missing Codex stdin")?);
        let mut stderr = child.stderr.take().ok_or("Missing Codex stderr")?;
        let stderr_task = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        });
        let mut process = Process {
            child,
            reader,
            writer,
            next_id: 1,
            login_completion: None,
            stderr_task,
        };
        process.request("initialize", json!({"clientInfo":{"name":"hive_desktop","title":"Hive","version":env!("CARGO_PKG_VERSION")}})).await?;
        process
            .writer
            .write_value(&json!({"method":"initialized"}))
            .await
            .map_err(|_| "Cannot initialize Codex")?;
        let result = process
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        let account = result.get("account").ok_or("Missing account response")?;
        self.status = account_status(account)?;
        self.process = Some(process);
        self.pending = None;
        Ok(())
    }

    async fn refresh(&mut self) -> Result<(), String> {
        let process = self
            .process
            .as_mut()
            .ok_or("Start the account connection first")?;
        let result = process
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        if let Some((id, since)) = &self.pending {
            if since.elapsed() > Duration::from_secs(15 * 60) {
                return self.cancel().await;
            }
            if let Some(event) = &process.login_completion {
                if event.get("loginId").and_then(Value::as_str) == Some(id.as_str()) {
                    if event.get("success").and_then(Value::as_bool) != Some(true) {
                        return Err(
                            "ChatGPT sign-in did not complete. Retry or use device sign-in.".into(),
                        );
                    }
                    let next =
                        account_status(result.get("account").ok_or("Missing account response")?)?;
                    if next.state == "connected" {
                        self.status = next;
                        self.pending = None;
                    }
                }
            }
        } else if self.status.state == "connected" {
            self.status = account_status(result.get("account").ok_or("Missing account response")?)?;
        }
        Ok(())
    }
    async fn login(&mut self, device: bool) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("A sign-in is already in progress. Cancel it first.".into());
        }
        let process = self
            .process
            .as_mut()
            .ok_or("Start the account connection first")?;
        process.login_completion = None;
        let kind = if device {
            "chatgptDeviceCode"
        } else {
            "chatgpt"
        };
        let response = process
            .request("account/login/start", json!({"type":kind}))
            .await?;
        if response.get("type").and_then(Value::as_str) != Some(kind) {
            return Err("Unexpected authentication method".into());
        }
        let id = response
            .get("loginId")
            .and_then(Value::as_str)
            .ok_or("Missing login attempt ID")?
            .to_owned();
        let url = response
            .get(if device { "verificationUrl" } else { "authUrl" })
            .and_then(Value::as_str)
            .ok_or("Missing sign-in URL")?;
        if !allowed_auth_url(url) {
            return Err("Codex returned an unrecognized sign-in address".into());
        }
        self.pending = Some((id, Instant::now()));
        self.status = AccountStatus {
            state: "signing_in".into(),
            auth_url: Some(url.to_owned()),
            user_code: response
                .get("userCode")
                .and_then(Value::as_str)
                .map(str::to_owned),
            detail: "Finish signing in using your browser. Hive will check for completion.".into(),
            ..Default::default()
        };
        Ok(())
    }
    async fn cancel(&mut self) -> Result<(), String> {
        let pending = self.pending.take();
        self.status = AccountStatus {
            state: "signed_out".into(),
            detail: "Sign-in cancelled.".into(),
            ..Default::default()
        };
        if let Some((id, _)) = pending {
            self.process
                .as_mut()
                .ok_or("Codex stopped")?
                .request("account/login/cancel", json!({"loginId":id}))
                .await?;
            // Clear any account stored during a race with completion.
            self.logout().await?;
        }
        Ok(())
    }
    async fn logout(&mut self) -> Result<(), String> {
        self.pending = None;
        self.status = AccountStatus {
            state: "signed_out".into(),
            detail: "Disconnected from ChatGPT.".into(),
            ..Default::default()
        };
        self.process
            .as_mut()
            .ok_or("Codex stopped")?
            .request("account/logout", json!({}))
            .await?;
        let result = self
            .process
            .as_mut()
            .unwrap()
            .request("account/read", json!({"refreshToken":false}))
            .await?;
        if !result.get("account").is_some_and(Value::is_null) {
            return Err("Codex has not confirmed logout. Retry disconnect.".into());
        }
        Ok(())
    }
}

/// One local OS user's connection shared by desktop views. No secrets cross this boundary.
pub async fn account_action(action: &str, binary: Option<String>) -> AccountStatus {
    let mut guard = ACCOUNT_CONNECTION.lock().await;
    let connection = guard.get_or_insert_with(AccountConnection::default);
    let result = async {
        if !matches!(
            action,
            "connect" | "device" | "status" | "cancel" | "disconnect"
        ) {
            return Err("Unknown account action".into());
        }
        if connection.process.is_none() {
            if action == "status" {
                return Ok(());
            }
            let executable = binary
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .or_else(discover_binary)
                .ok_or("Install Codex 0.149.0 or choose its executable in Advanced.")?;
            let home = dirs::data_local_dir()
                .ok_or("No local application directory")?
                .join("OHHive/subscriptions/chatgpt");
            connection.start(&executable, &home).await?;
        }
        match action {
            "connect" if connection.status.state == "connected" => Ok(()),
            "connect" => connection.login(false).await,
            "device" => connection.login(true).await,
            "status" => connection.refresh().await,
            "cancel" => connection.cancel().await,
            "disconnect" => connection.logout().await,
            _ => Err("Unknown account action".into()),
        }
    }
    .await;
    if let Err(detail) = result {
        connection.process = None;
        connection.pending = None;
        connection.status = AccountStatus {
            state: "error".into(),
            detail,
            ..Default::default()
        };
    }
    connection.status.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_urls_reject_other_authorities() {
        assert!(allowed_auth_url(
            "https://auth.openai.com/authorize?state=test"
        ));
        for url in [
            "https://auth.openai.com.evil.test/",
            "https://auth.openai.com@evil.test/",
            "javascript:alert(1)",
            "http://auth.openai.com/",
        ] {
            assert!(!allowed_auth_url(url));
        }
    }
    #[test]
    fn account_read_requires_managed_chatgpt() {
        assert!(account_status(&json!({"type":"apiKey"})).is_err());
        assert_eq!(account_status(&Value::Null).unwrap().state, "signed_out");
        assert_eq!(
            account_status(&json!({"type":"chatgpt","email":"test@example.com","planType":"plus"}))
                .unwrap()
                .state,
            "connected"
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn managed_login_correlation_visibility_cancel_and_logout() {
        use std::os::unix::fs::PermissionsExt;
        let folder =
            std::env::temp_dir().join(format!("hive-account-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let binary = folder.join("codex-fixture");
        std::fs::write(&binary, include_str!("fixtures/account_server.py")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut connection = AccountConnection::default();
        connection
            // This verifies protocol transitions, not interpreter startup speed on a busy
            // shared runner. Production retains five seconds; the deadline has its own test.
            .start_with_version_timeout(&binary, &folder.join("home"), Duration::from_secs(30))
            .await
            .expect("local Python account fixture must start (no installed Codex required)");
        connection.login(false).await.unwrap();
        connection.refresh().await.unwrap();
        assert_eq!(
            connection.status.state, "signing_in",
            "stale completion must not activate account"
        );
        connection.refresh().await.unwrap();
        assert_eq!(
            connection.status.state, "signing_in",
            "completion before account visibility remains pending"
        );
        connection.refresh().await.unwrap();
        assert_eq!(connection.status.state, "connected");
        connection.logout().await.unwrap();
        assert_eq!(connection.status.state, "signed_out");
        connection.login(true).await.unwrap();
        assert_eq!(connection.status.user_code.as_deref(), Some("FIXTURE-CODE"));
        connection.cancel().await.unwrap();
        connection.refresh().await.unwrap();
        assert_eq!(
            connection.status.state, "signed_out",
            "cancel/logout clears racing completion"
        );
        assert!(connection.status.auth_url.is_none());
        assert!(connection.pending.is_none());
        drop(connection);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stalled_version_probe_returns_error_without_starting_session() {
        use std::os::unix::fs::PermissionsExt;
        let folder = std::env::temp_dir().join(format!("hive-stalled-version-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let binary = folder.join("stalled-codex");
        // exec ensures the child killed by kill_on_drop is the sleeper, not a shell parent.
        std::fs::write(&binary, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut connection = AccountConnection::default();
        let error = connection.start_with_version_timeout(
            &binary, &folder.join("home"), Duration::from_millis(100),
        ).await.unwrap_err();
        assert_eq!(error, "Codex version check timed out");
        assert!(connection.process.is_none());
        assert!(connection.pending.is_none());
        assert_eq!(connection.status.state, "not_started");
        drop(connection);
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[tokio::test]
    #[ignore = "Requires installed pinned Codex; no login or inference"]
    async fn real_signed_out_handshake() {
        let folder =
            std::env::temp_dir().join(format!("hive-account-probe-{}", uuid::Uuid::new_v4()));
        let binary = discover_binary().expect("Codex installed");
        let mut connection = AccountConnection::default();
        connection.start(&binary, &folder).await.unwrap();
        assert_eq!(connection.status.state, "signed_out");
        connection.login(false).await.unwrap();
        assert_eq!(connection.status.state, "signing_in");
        assert!(connection.status.auth_url.as_deref().is_some_and(allowed_auth_url));
        connection.cancel().await.unwrap();
        assert_eq!(connection.status.state, "signed_out");
        drop(connection);
        let _ = std::fs::remove_dir_all(folder);
    }
}
