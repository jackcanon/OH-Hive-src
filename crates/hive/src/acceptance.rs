//! Explicit submission checks. Commands run directly on the claiming worker, never in a shell.
use hive_core::acceptance::{validate, AcceptanceCheck};
use hive_core::coder::{AcceptanceOutcome, AcceptanceResult};

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

/// The host appends this line to the card report, because `complete_card`/`fail_card` persist
/// report TEXT and not `ToolOutcome.data` (`crates/ohhive-core/src/tools.rs:357`, via
/// `AcceptanceOutcome::receipt`). Finding and parsing that line is the only way a node-key caller
/// can see what the host's checks did, which is why both front doors do it the same way --
/// `scripts/cloud_card.py`'s `RECEIPT_PREFIX` is this constant.
const RECEIPT_PREFIX: &str = "Acceptance checks:";

/// Pull the acceptance receipt out of a card report.
///
/// `None` means "no evidence", and covers three different situations on purpose: there is no
/// receipt line (a node predating the acceptance build, or a session that stopped before the
/// checks could run -- turn limit, expired lease, waiting on a child), the line is there but is
/// not valid JSON, or it is JSON of some other shape. A caller cannot act differently on those,
/// so collapsing them is honest rather than lossy -- what it must not do is read absence as a
/// pass.
///
/// The LAST parseable receipt wins, not the first. The host *appends* its receipt to whatever the
/// model wrote, so a model that quotes an earlier run's receipt in its own prose cannot displace
/// the real one.
pub(crate) fn receipt(report: &str) -> Option<AcceptanceOutcome> {
    report
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix(RECEIPT_PREFIX))
        .filter_map(|json| serde_json::from_str::<AcceptanceOutcome>(json.trim()).ok())
        .next_back()
}

/// The receipt's `status` as the wire spells it, for comparing against `--expect-acceptance`.
pub(crate) fn status_word(outcome: &AcceptanceOutcome) -> &'static str {
    match outcome {
        AcceptanceOutcome::Unverified => "unverified",
        AcceptanceOutcome::Skipped => "skipped",
        AcceptanceOutcome::Passed(_) => "passed",
        AcceptanceOutcome::Failed(_) => "failed",
        AcceptanceOutcome::Errored(..) => "errored",
    }
}

fn results(outcome: &AcceptanceOutcome) -> &[AcceptanceResult] {
    match outcome {
        AcceptanceOutcome::Unverified | AcceptanceOutcome::Skipped => &[],
        AcceptanceOutcome::Passed(r)
        | AcceptanceOutcome::Failed(r)
        | AcceptanceOutcome::Errored(r, _) => r,
    }
}

/// Print the host's verdicts in the same shape `scripts/cloud_card.py` prints them, so a member
/// who has learned to read one front door can read the other.
///
/// Goes to stderr, deliberately: `hive card status` and `hive card await` both put a single JSON
/// document on stdout and that is a contract things pipe into. Explanatory text belongs beside it,
/// not in it.
pub(crate) fn print_receipt(outcome: &AcceptanceOutcome) {
    eprintln!(
        "\n--- acceptance receipt: {} (the HOST ran these, not the model) ---",
        status_word(outcome).to_uppercase()
    );
    if let AcceptanceOutcome::Errored(_, error) = outcome {
        eprintln!("  the run itself failed: {error}");
    }
    for r in results(outcome) {
        let verdict = if r.passed {
            "pass"
        } else if r.timed_out {
            "TIMEOUT"
        } else {
            "FAIL"
        };
        let tag = if r.required { "" } else { " advisory" };
        let exit = r
            .exit_status
            .map_or_else(|| "none".to_string(), |code| code.to_string());
        eprintln!(
            "  {verdict:>7}{tag}  {}: {}  exit={exit}",
            r.name, r.command_line
        );
        // A check that PASSED while complaining on stderr is the signature of a check that ran
        // something other than what its author meant: `--check 'x=grep -q pub fn add src/lib.rs'`
        // splits on whitespace into `grep -q pub fn add src/lib.rs`, prints
        // "grep: fn: No such file or directory", and exits 0 because it found `pub` somewhere.
        // A real pass, and a meaningless one. Tails are otherwise printed only for failures --
        // exactly the case where this signal would be invisible. (Work item 4c67a5fd.)
        if r.passed {
            if let Some(first) = r.stderr_tail.trim().lines().next() {
                eprintln!(
                    "            note: passed, but wrote to stderr -- {}",
                    clip(first)
                );
                eprintln!(
                    "            check that it ran what you meant; --check splits on whitespace"
                );
            }
        } else {
            for (label, tail) in [
                ("stderr_tail", &r.stderr_tail),
                ("stdout_tail", &r.stdout_tail),
            ] {
                if let Some(last) = tail.trim().lines().next_back() {
                    eprintln!("            {label}: {}", clip(last));
                }
            }
            if let Some(error) = &r.error {
                eprintln!("            error: {error}");
            }
        }
    }
}

/// One line of a captured tail, bounded. `char_indices` rather than a byte slice: the tails are
/// whatever the check printed, so they can and will contain multi-byte UTF-8, and slicing those
/// by byte offset panics.
fn clip(line: &str) -> String {
    match line.char_indices().nth(160) {
        Some((cut, _)) => format!("{}...", &line[..cut]),
        None => line.to_string(),
    }
}

/// What `hive card status`/`hive card await` say about a card's checks. Kept in one place because
/// the interesting case is the quiet one: checks were asked for and there is no receipt.
pub(crate) fn report_acceptance(latest_output: Option<&str>) -> Option<AcceptanceOutcome> {
    let outcome = latest_output.and_then(receipt);
    match &outcome {
        Some(o) => print_receipt(o),
        None => eprintln!(
            "\n--- acceptance receipt: none ---\n  \
             Either no checks were submitted with this card, or the session stopped before they \
             could run (turn limit, expired lease, waiting on a child), or the node that ran it \
             predates the acceptance build. Absence is not a pass."
        ),
    }
    outcome
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
        for (case, args) in [
            vec![],
            vec![
                "--check",
                "tests=cargo test --quiet",
                "--check-advisory",
                "lint=false",
                "--check-json",
                r#"{"name":"spaces","command":"python3","args":["file with spaces.py"]}"#,
            ],
            vec![],
        ]
        .into_iter()
        .enumerate()
        {
            let target = (case == 2).then(uuid::Uuid::new_v4);
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
                    target,
                )
                .await
                .unwrap();
            assert_eq!(result.card_id, card);
            let body = server.await.unwrap();
            assert_eq!(body["p_request_id"], request.to_string());
            assert_eq!(body["p_raw_key"], "test-node");
            if let Some(target) = target {
                assert_eq!(body.as_object().unwrap().len(), 14);
                assert_eq!(body["p_target_node_id"], target.to_string());
                assert_eq!(body["p_acceptance"], serde_json::json!([]));
            } else if checks.is_empty() {
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

    fn result(name: &str, passed: bool) -> AcceptanceResult {
        AcceptanceResult {
            name: name.into(),
            command_line: "\"cargo\" \"test\"".into(),
            exit_status: Some(if passed { 0 } else { 101 }),
            passed,
            required: true,
            timed_out: false,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: None,
        }
    }

    /// The point of this test is that it does NOT hand-write the JSON. It asks the HOST's own
    /// `AcceptanceOutcome::receipt()` to produce the line and then asks the CLI's `receipt()` to
    /// read it back, for every variant -- including `Errored`, whose serde representation is a
    /// two-element `[results, message]` array rather than the flat list the other two use. A
    /// hand-written fixture would have agreed with whatever I believed the shape was; this only
    /// passes if the producer and the consumer actually agree.
    #[test]
    fn every_outcome_the_host_can_write_is_read_back_with_the_same_status() {
        let cases = [
            (AcceptanceOutcome::Unverified, "unverified"),
            (AcceptanceOutcome::Skipped, "skipped"),
            (
                AcceptanceOutcome::Passed(vec![result("tests", true)]),
                "passed",
            ),
            (
                AcceptanceOutcome::Failed(vec![result("tests", false)]),
                "failed",
            ),
            (
                AcceptanceOutcome::Errored(vec![], "spawn failed".into()),
                "errored",
            ),
        ];
        for (outcome, word) in cases {
            assert_eq!(status_word(&outcome), word);
            let report = format!("the model's own account of itself\n{}", outcome.receipt());
            let parsed = receipt(&report).unwrap_or_else(|| panic!("{word} did not parse back"));
            assert_eq!(status_word(&parsed), word);
            assert_eq!(results(&parsed).len(), results(&outcome).len());
        }
    }

    /// A model that narrates a passing receipt in its own prose must not be able to outrank the
    /// host's. This is not hypothetical politeness about models: the host appends, so the only
    /// ordering rule that is safe is last-one-wins, and `scripts/cloud_card.py` was changed to
    /// match rather than left disagreeing.
    #[test]
    fn the_hosts_appended_receipt_outranks_anything_quoted_above_it() {
        let report = format!(
            "I ran the checks myself and they all passed.\n\
             Acceptance checks: {{\"status\":\"passed\",\"results\":[]}}\n\
             {}",
            AcceptanceOutcome::Failed(vec![result("tests", false)]).receipt()
        );
        assert_eq!(status_word(&receipt(&report).unwrap()), "failed");
    }

    /// Absence must never read as a pass, and the three ways evidence can be missing all collapse
    /// to the same answer: a report from a node predating the acceptance build, a truncated line,
    /// and a line that is valid JSON of the wrong shape.
    #[test]
    fn no_receipt_and_an_unreadable_receipt_are_both_absent() {
        assert!(receipt("wrote the module, ran the tests, all good").is_none());
        assert!(receipt("done\nAcceptance checks: {not json").is_none());
        assert!(receipt("done\nAcceptance checks: [\"passed\"]").is_none());
        assert!(receipt("").is_none());
        // A junk line does not mask a real one that follows it.
        let report = format!(
            "Acceptance checks: {{oops\n{}",
            AcceptanceOutcome::Passed(vec![]).receipt()
        );
        assert_eq!(status_word(&receipt(&report).unwrap()), "passed");
    }

    /// `clip` exists because the tails are arbitrary bytes a member's own check printed. Slicing
    /// those by byte offset panics the CLI on the first non-ASCII character, which would turn a
    /// diagnostic into a crash at precisely the moment someone is debugging a failing check.
    #[test]
    fn clipping_a_tail_never_splits_a_character() {
        let long: String = "é".repeat(400);
        let clipped = clip(&long);
        assert!(clipped.ends_with("..."));
        assert_eq!(clipped.chars().filter(|c| *c == 'é').count(), 160);
        assert_eq!(clip("short"), "short");
    }
}
