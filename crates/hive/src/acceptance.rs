//! Explicit submission checks. Commands run directly on the claiming worker, never in a shell.
use hive_core::acceptance::{validate, AcceptanceCheck};

#[derive(clap::Args)]
pub(crate) struct CheckArgs {
    /// Required check, repeatable: 'NAME=PROGRAM ARG...'. Split on whitespace, no shell
    /// expansion or quote parsing. Use --check-json for arguments containing spaces.
    #[arg(long = "check", value_name = "NAME=PROGRAM ARG...", value_parser = required_check)]
    required: Vec<AcceptanceCheck>,
    /// Advisory check with the same syntax as --check; a failure is recorded but does not
    /// block completion.
    #[arg(long = "check-advisory", value_name = "NAME=PROGRAM ARG...", value_parser = advisory_check)]
    advisory: Vec<AcceptanceCheck>,
    /// One check as JSON, repeatable. Fields: name, command, args, cwd, expect_exit, required.
    /// Defaults: args=[], workspace root, exit 0, required=true. Commands run on the worker.
    #[arg(long = "check-json", value_name = "JSON", value_parser = json_check)]
    json: Vec<AcceptanceCheck>,
}

impl CheckArgs {
    pub(crate) fn into_checks(self) -> anyhow::Result<Vec<AcceptanceCheck>> {
        // Match cloud_card.py: required shorthand, advisory shorthand, then JSON checks.
        let checks: Vec<_> = self
            .required
            .into_iter()
            .chain(self.advisory)
            .chain(self.json)
            .collect();
        validate(&checks).map_err(anyhow::Error::msg)?;
        Ok(checks)
    }
}

fn shorthand(value: &str, required: bool) -> Result<AcceptanceCheck, String> {
    let (name, command) = value
        .split_once('=')
        .ok_or("expected NAME=PROGRAM ARG...")?;
    let mut words = command.split_whitespace();
    let check = AcceptanceCheck {
        name: name.trim().into(),
        command: words
            .next()
            .ok_or("check needs a program after '='")?
            .into(),
        args: words.map(str::to_owned).collect(),
        cwd: None,
        expect_exit: 0,
        required,
    };
    validate(std::slice::from_ref(&check)).map_err(str::to_owned)?;
    Ok(check)
}
fn required_check(value: &str) -> Result<AcceptanceCheck, String> {
    shorthand(value, true)
}
fn advisory_check(value: &str) -> Result<AcceptanceCheck, String> {
    shorthand(value, false)
}
fn json_check(value: &str) -> Result<AcceptanceCheck, String> {
    let check =
        serde_json::from_str(value).map_err(|error| format!("invalid check JSON: {error}"))?;
    validate(std::slice::from_ref(&check)).map_err(str::to_owned)?;
    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    fn parse(args: &[&str]) -> anyhow::Result<Vec<AcceptanceCheck>> {
        let cli = crate::Cli::try_parse_from(
            [
                "hive",
                "card",
                "submit",
                "--project",
                "demo",
                "--task",
                "test",
                "--workspace",
                "/tmp/demo",
            ]
            .into_iter()
            .chain(args.iter().copied()),
        )?;
        let crate::Cmd::Card {
            cmd: crate::CardCmd::Submit { checks, .. },
        } = cli.cmd
        else {
            unreachable!()
        };
        (*checks).into_checks()
    }
    #[test]
    fn cli_checks_preserve_direct_argv_and_defaults() {
        let checks = parse(&["--check", "tests=cargo test --quiet", "--check", "lint=cargo fmt --check",
            "--check-advisory", "style=false", "--check-json",
            r#"{"name":"path","command":"python3","args":["test with spaces.py","a=b"],"cwd":"sub dir","expect_exit":2,"required":false}"#]).unwrap();
        assert_eq!(checks.len(), 4);
        assert_eq!(checks[0].args, ["test", "--quiet"]);
        assert!(checks[0].required);
        assert_eq!(checks[0].expect_exit, 0);
        assert!(!checks[2].required);
        assert_eq!(checks[3].args, ["test with spaces.py", "a=b"]);
        assert_eq!(checks[3].cwd.as_deref(), Some("sub dir"));
        assert_eq!(checks[3].expect_exit, 2);
        assert!(parse(&[]).unwrap().is_empty());
    }
    #[test]
    fn malformed_or_excessive_checks_fail_before_submission() {
        for value in ["cargo test", "=cargo test", "tests=", " =cargo"] {
            assert!(parse(&["--check", value]).is_err(), "{value}");
        }
        for value in [
            r#"{"name":"x","command":"true","shell":true}"#,
            r#"{"name":"x","command":"true","args":"test"}"#,
            r#"{"name":"x","command":"true","expect_exit":1.5}"#,
            "[]",
            "{}",
        ] {
            assert!(parse(&["--check-json", value]).is_err(), "{value}");
        }
        let args: Vec<_> = ["--check", "x=true"].into_iter().cycle().take(34).collect();
        assert!(parse(&args).is_err());
        assert!(parse(&args[..32]).is_ok());
    }
    #[tokio::test]
    async fn cli_checks_reach_submission_rpc_without_changing_legacy_requests() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for args in [
            vec![],
            vec![
                "--check",
                "tests=cargo test --quiet",
                "--check-advisory",
                "lint=false",
                "--check-json",
                r#"{"name":"spaces","command":"python3","args":["file with spaces.py"]}"#,
            ],
        ] {
            let checks = parse(&args).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let project = uuid::Uuid::new_v4();
            let card = uuid::Uuid::new_v4();
            let request = uuid::Uuid::new_v4();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let (header_end, length) = loop {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert_ne!(n, 0);
                    raw.extend_from_slice(&buf[..n]);
                    if let Some(end) = raw.windows(4).position(|s| s == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..end]);
                        assert!(
                            headers.starts_with("POST /rest/v1/rpc/hive_code_session_create_node ")
                        );
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        break (end + 4, length);
                    }
                };
                while raw.len() < header_end + length {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    assert_ne!(n, 0);
                    raw.extend_from_slice(&buf[..n]);
                }
                let body: serde_json::Value =
                    serde_json::from_slice(&raw[header_end..header_end + length]).unwrap();
                let response = serde_json::json!({"card_id":card,"project_id":project}).to_string();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).as_bytes()).await.unwrap();
                body
            });
            let client = hive_core::hub::HubClient::new(
                format!("http://{address}"),
                "test-anon",
                "test-node",
            );
            let result = client
                .code_session_submit(
                    project,
                    "task",
                    Some("/tmp/demo"),
                    None,
                    None,
                    "local",
                    None,
                    3,
                    false,
                    Some(request),
                    false,
                    &checks,
                )
                .await
                .unwrap();
            assert_eq!(result.card_id, card);
            let body = server.await.unwrap();
            assert_eq!(body["p_request_id"], request.to_string());
            assert_eq!(body["p_raw_key"], "test-node");
            if checks.is_empty() {
                assert_eq!(body.as_object().unwrap().len(), 12);
                assert!(body.get("p_acceptance").is_none());
            } else {
                assert_eq!(body.as_object().unwrap().len(), 13);
                let wire = body["p_acceptance"].as_array().unwrap();
                assert_eq!(wire.len(), 3);
                assert_eq!(wire[0]["command"], "cargo");
                assert_eq!(wire[0]["args"], serde_json::json!(["test", "--quiet"]));
                // The deployed SQL validator rejects explicit null cwd. Omit when absent.
                assert!(wire.iter().all(|check| check.get("cwd").is_none()));
                assert_eq!(wire[1]["required"], false);
                assert_eq!(wire[2]["args"], serde_json::json!(["file with spaces.py"]));
            }
        }
    }
}
