//! One-shot GitHub read check. No token is persisted or installed as a worker-wide credential.
use super::run_git_command;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::{path::Path, process::Stdio};

const FAILED: &str = "Git could not read this repository. Check GitHub access, repository contents permission, and your connection.";

fn validate(repo: &str, token: &str) -> Result<(), &'static str> {
    let path = repo
        .strip_prefix("https://github.com/")
        .ok_or("Use a GitHub HTTPS repository URL")?;
    let parts: Vec<_> = path.split('/').collect();
    if path.len() > 300
        || parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || !p
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        })
    {
        return Err("Invalid GitHub repository URL");
    }
    if token.is_empty()
        || token.len() > 4096
        || !token
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err("Reconnect GitHub before checking repository access");
    }
    Ok(())
}

fn command(repo: &str, token: &str, cwd: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("git");
    // Clear tracing, credential helpers, proxy overrides, injected config, and repo discovery
    // settings inherited from the app. Keep only trusted OS executable lookup requirements.
    cmd.env_clear();
    for key in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    cmd.env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_SYSTEM", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CEILING_DIRECTORIES", cwd.parent().unwrap_or(cwd));
    let config = [
        (
            format!("http.{repo}.extraHeader"),
            format!(
                "Authorization: Basic {}",
                STANDARD.encode(format!("x-access-token:{token}"))
            ),
        ),
        ("http.followRedirects".into(), "false".into()),
        ("http.sslVerify".into(), "true".into()),
        ("credential.helper".into(), "".into()),
        ("core.askPass".into(), "".into()),
        ("protocol.allow".into(), "never".into()),
        ("protocol.https.allow".into(), "always".into()),
    ];
    cmd.env("GIT_CONFIG_COUNT", config.len().to_string());
    for (i, (key, value)) in config.into_iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{i}"), key)
            .env(format!("GIT_CONFIG_VALUE_{i}"), value);
    }
    cmd.args(["ls-remote", "--", repo, "HEAD"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// Called only by a trusted host action, with the token held in memory for this operation.
/// A successful public-repository check does not prove private access or account identity.
pub async fn check_read_access(repo: &str, token: &str) -> Result<(), &'static str> {
    validate(repo, token)?;
    let scratch = std::env::temp_dir().join(format!("hive-git-check-{}", uuid::Uuid::new_v4()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&scratch)
        .map_err(|_| "Cannot prepare Git access check")?;
    let scratch = Scratch(scratch);
    let output = run_git_command(
        command(repo, token, &scratch.0),
        &["ls-remote", repo, "HEAD"],
        true,
    )
    .await
    .map_err(|_| FAILED)?;
    validate_output(&output)
}

fn validate_output(output: &str) -> Result<(), &'static str> {
    // Empty repositories have no HEAD. Never return remote text or a token-bearing diagnostic.
    if output.is_empty() {
        return Ok(());
    }
    let line = output.trim_end_matches('\n');
    let Some((sha, name)) = line.split_once('\t') else {
        return Err(FAILED);
    };
    if name != "HEAD"
        || ![40, 64].contains(&sha.len())
        || !sha.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err(FAILED);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn subprocess_errors_never_return_credential_output() {
        let mut cmd = tokio::process::Command::new("/bin/sh");
        cmd.args(["-c", "printf '%s' \"$SYNTHETIC_TOKEN\" >&2; exit 7"])
            .env("SYNTHETIC_TOKEN", "ghu_fixture_should_not_escape")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let error = run_git_command(cmd, &["ls-remote", "HEAD"], true)
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(!message.contains("ghu_fixture_should_not_escape"));
        assert!(message.contains("Authenticated Git request failed"));
    }

    #[test]
    fn credentials_are_scoped_and_absent_from_arguments() {
        let repo = "https://github.com/example/private.git";
        let cmd = command(repo, "ghu_synthetic", Path::new("/tmp/probe"));
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy())
            .collect();
        assert_eq!(args, ["ls-remote", "--", repo, "HEAD"]);
        let env: std::collections::HashMap<_, _> = cmd
            .as_std()
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.unwrap().to_string_lossy().into_owned(),
                )
            })
            .collect();
        assert_eq!(env["GIT_CONFIG_KEY_0"], format!("http.{repo}.extraHeader"));
        assert_eq!(env["GIT_CONFIG_VALUE_1"], "false");
        assert_eq!(env["GIT_CONFIG_VALUE_2"], "true");
        assert_eq!(env["GIT_CONFIG_VALUE_3"], "");
        assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
        assert!(!env.contains_key("GIT_TRACE"));
    }
    #[test]
    fn rejects_redirect_shapes_credentials_and_untrusted_output() {
        for url in [
            "http://github.com/o/r",
            "https://github.com.evil.test/o/r",
            "https://token@github.com/o/r",
            "https://github.com/o/r?token=x",
            "https://github.com/../r",
            "https://github.com/o/r/extra",
        ] {
            assert!(validate(url, "synthetic").is_err());
        }
        assert!(validate("https://github.com/o/r", "bad\nheader").is_err());
        assert!(validate_output("unexpected secret-bearing output").is_err());
        assert!(validate_output(&format!("{}\tHEAD\n", "a".repeat(40))).is_ok());
        assert!(validate_output("").is_ok());
    }
}
