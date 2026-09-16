//! ADR-034 P2 compile-time contract; not a shipping account adapter.
use github_copilot_sdk::{
    handler::DenyAllHandler, mode::ClientMode, Client, ClientOptions, ResumeSessionConfig,
    SessionConfig, Transport,
};
use std::{ffi::OsString, path::Path, sync::Arc};

/// Use only the token supplied by Hive's selected account. No keychain lookup here.
/// Caller must supply an owner-protected, dedicated directory, never a project folder.
pub fn options(token: String, home: &Path) -> Result<ClientOptions, &'static str> {
    if !token.starts_with("ghu_") || token.len() <= 4 {
        return Err("A GitHub App user token is required");
    }
    if !home.is_absolute() {
        return Err("An absolute, private runtime directory is required");
    }
    // Remove inherited configuration, but preserve platform process necessities.
    // Do NOT remove COPILOT_SDK_AUTH_TOKEN: the SDK injects our explicit token
    // before applying env_remove. Removing it would silently erase our selection.
    let keep = [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TMPDIR",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
    ];
    let remove: Vec<OsString> = std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| {
            !keep.iter().any(|allowed| key == allowed)
                && key != "COPILOT_SDK_AUTH_TOKEN"
                && key != "COPILOT_HOME"
                && key != "COPILOT_DISABLE_KEYTAR"
        })
        .collect();
    Ok(ClientOptions::default()
        .with_github_token(token)
        .with_use_logged_in_user(false)
        .with_mode(ClientMode::Empty)
        .with_base_directory(home)
        .with_env_remove(remove)
        .with_cwd(home)
        .with_transport(Transport::Stdio))
}

/// Fail closed when the runtime cannot confirm the app-selected account.
pub fn identity_matches(authenticated: bool, observed: Option<&str>, expected: &str) -> bool {
    authenticated
        && !expected.is_empty()
        && observed.is_some_and(|login| login.eq_ignore_ascii_case(expected))
}

/// Empty tool allowlist plus an explicit deny handler; no automatic approvals.
pub fn session(id: &str, model: &str, workspace: &Path) -> SessionConfig {
    SessionConfig::default()
        .with_session_id(id)
        .with_model(model)
        .with_client_name("Loki's Den")
        .with_working_directory(workspace)
        .with_streaming(true)
        .with_available_tools(Vec::<String>::new())
        .with_permission_handler(Arc::new(DenyAllHandler))
}

pub fn resume(id: &str, workspace: &Path) -> ResumeSessionConfig {
    ResumeSessionConfig::new(id.into())
        .with_client_name("Loki's Den")
        .with_working_directory(workspace)
        .with_streaming(true)
        .with_available_tools(Vec::<String>::new())
        .with_permission_handler(Arc::new(DenyAllHandler))
}

/// Conformance for the scoped stdio bridge. This config exposes no tools yet;
/// P3 must bind an authorized broker and explicit tool names before enabling any.
pub fn with_disabled_bridge(config: SessionConfig, executable: &Path) -> SessionConfig {
    use github_copilot_sdk::types::{McpServerConfig, McpStdioServerConfig};
    let bridge = McpServerConfig::Stdio(McpStdioServerConfig {
        command: executable.to_string_lossy().into_owned(),
        tools: Some(Vec::new()),
        timeout: Some(30_000),
        ..Default::default()
    });
    config.with_mcp_servers([(String::from("hive"), bridge)].into_iter().collect())
}

pub async fn resume_contract(
    client: &Client,
    config: ResumeSessionConfig,
) -> github_copilot_sdk::Result<()> {
    let session = client.resume_session(config).await?;
    let _events = session.subscribe();
    session.disconnect().await
}

/// Compiled lifecycle surface only. Never invoked by tests; requires a real bound account.
/// Production must add journal/fencing and terminal-event reconciliation before sending.
pub async fn lifecycle_contract(
    client: &Client,
    config: SessionConfig,
) -> github_copilot_sdk::Result<()> {
    let _auth = client.get_auth_status().await?;
    let _models = client.list_models().await?;
    let session = client.create_session(config).await?;
    let _events = session.subscribe(); // register before sending
    let _message = session.send("Hive compatibility probe").await?;
    session.abort().await?;
    session.disconnect().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_mismatch_and_missing_identity_fail_closed() {
        assert!(identity_matches(true, Some("JackCanon"), "jackcanon"));
        assert!(!identity_matches(false, Some("jackcanon"), "jackcanon"));
        assert!(!identity_matches(true, None, "jackcanon"));
        assert!(!identity_matches(true, Some("someone-else"), "jackcanon"));
        assert!(!identity_matches(true, Some(""), ""));
    }
    #[test]
    fn refuses_other_credential_modes() {
        for token in ["", "ghu_", "ghp_dummy", "ghs_dummy", "github_pat_dummy"] {
            assert!(options(token.into(), &std::env::temp_dir().join("hive-conformance")).is_err());
        }
        assert!(options("ghu_test_only".into(), Path::new("relative")).is_err());
    }
    #[test]
    fn explicit_identity_isolated_and_redacted() {
        let config = options(
            "ghu_test_only".into(),
            &std::env::temp_dir().join("hive-conformance"),
        )
        .unwrap();
        assert_eq!(config.use_logged_in_user, Some(false));
        assert_eq!(config.mode, ClientMode::Empty);
        assert!(!config
            .env_remove
            .contains(&OsString::from("COPILOT_SDK_AUTH_TOKEN")));
        assert!(!format!("{config:?}").contains("ghu_test_only"));
    }
    #[test]
    fn bridge_defaults_to_no_tools() {
        let config = with_disabled_bridge(
            session(
                "hive-test",
                "test-model",
                &std::env::temp_dir().join("hive-conformance"),
            ),
            Path::new("/private/hive/bridge"),
        );
        let bridge = &config.mcp_servers.unwrap()["hive"];
        let value = serde_json::to_value(bridge).unwrap();
        assert_eq!(value["type"], "stdio");
        assert_eq!(value["tools"], serde_json::json!([]));
    }
    #[test]
    fn create_and_resume_keep_permission_handler() {
        let create = session(
            "hive-test",
            "test-model",
            &std::env::temp_dir().join("hive-conformance"),
        );
        let resume = resume("hive-test", &std::env::temp_dir().join("hive-conformance"));
        assert!(create.permission_handler.is_some());
        assert!(resume.permission_handler.is_some());
        assert_eq!(create.available_tools, Some(Vec::new()));
        assert_eq!(resume.available_tools, Some(Vec::new()));
    }
}
