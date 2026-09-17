use super::*;
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("hive-acceptance-{}", Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn check(script: &str, required: bool, exit: i32) -> AcceptanceCheck {
    AcceptanceCheck {
        name: "unit tests".into(),
        command: "python3".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        expect_exit: exit,
        required,
    }
}
async fn execute(checks: Vec<AcceptanceCheck>, timeout: Duration) -> AcceptanceOutcome {
    let w = Workspace::new();
    acceptance::run(
        &NoopHub,
        Uuid::new_v4(),
        &w.0,
        &checks,
        chrono::Utc::now() + chrono::Duration::minutes(2),
        timeout,
    )
    .await
}
#[tokio::test]
async fn checks_gate_required_failures_advisory_and_expected_exit() {
    for (exit, required, expect, blocked) in [
        (0, true, 0, false),
        (1, true, 0, true),
        (1, false, 0, false),
        (1, true, 1, false),
    ] {
        let result = execute(
            vec![check(
                &format!("import sys;sys.exit({exit})"),
                required,
                expect,
            )],
            Duration::from_secs(5),
        )
        .await;
        assert_eq!(result.blocks_completion(), blocked, "{result:?}");
        assert!(result.receipt().contains("exit_status"));
    }
    assert!(matches!(
        execute(vec![], Duration::from_secs(1)).await,
        AcceptanceOutcome::Unverified
    ));
}
#[tokio::test]
async fn missing_binary_and_invalid_cwd_are_errors() {
    let mut c = check("", true, 0);
    c.command = "hive-definitely-missing-executable".into();
    assert!(matches!(
        execute(vec![c], Duration::from_secs(2)).await,
        AcceptanceOutcome::Errored(..)
    ));
    let mut c = check("", true, 0);
    c.cwd = Some("../outside".into());
    assert!(matches!(
        execute(vec![c], Duration::from_secs(2)).await,
        AcceptanceOutcome::Errored(..)
    ));
}
#[tokio::test]
async fn timeout_preserves_bounded_output_tails() {
    // The timeout has to be long enough that the child certainly finished WRITING, and short
    // enough that it is certainly still SLEEPING when the deadline lands -- the assertions below
    // depend on both. At 400ms the first half was not reliable: this failed on ubuntu-latest in
    // run 35161142631 with exactly `stdout_tail.ends_with("TAIL\n")`, because the child was killed
    // mid-write and the tail is then a prefix of the output rather than its end. That is the
    // implementation behaving correctly -- it preserves what it read -- so the fix is the test's
    // timing assumption, not `read_output`. Reproduced deliberately before changing anything:
    // dropping 400ms to 15ms makes the identical assertion fail on macOS too, which is the same
    // race with more of it. 3s against a 60s sleep is ~50x python3's cold start on the slowest
    // runner we use and still ~20x clear of the sleep, so neither half is close.
    let result=execute(vec![check("import sys,time;print('x'*12000+'TAIL',flush=True);print('ERROR',file=sys.stderr,flush=True);time.sleep(60)",true,0)],Duration::from_secs(3)).await;
    let AcceptanceOutcome::Failed(results) = result else {
        panic!("{result:?}")
    };
    assert!(results[0].timed_out);
    assert!(results[0].stdout_tail.ends_with("TAIL\n"));
    assert!(results[0].stdout_tail.len() <= 4096);
    assert!(results[0].stderr_tail.contains("ERROR"));
}
struct Brain(bool);
#[async_trait::async_trait]
impl CodeBrain for Brain {
    async fn next_turn(
        &self,
        _: &[BrainMessage],
        _: &[ToolSpec],
    ) -> std::result::Result<BrainTurn, CodeBrainError> {
        if self.0 {
            Ok(BrainTurn::text("Done".into()))
        } else {
            Ok(BrainTurn::tool_calls(vec![]))
        }
    }
}
#[tokio::test]
async fn session_skips_process_on_turn_limit_and_expired_lease() {
    let w = Workspace::new();
    let spec=CodeSessionSpec::from_required_capabilities(&serde_json::json!({"task":"test","workspace_path":w.0,"max_turns":1,"acceptance":[check("open('RAN','w').write('yes')",true,0)]})).unwrap();
    for (done, lease) in [
        (false, chrono::Utc::now() + chrono::Duration::minutes(2)),
        (true, chrono::Utc::now() - chrono::Duration::seconds(1)),
    ] {
        let result = run_session(&NoopHub, &w.0, Uuid::new_v4(), &spec, &Brain(done), lease)
            .await
            .unwrap();
        assert!(matches!(result.acceptance, AcceptanceOutcome::Skipped));
        assert!(!w.0.join("RAN").exists());
    }
}
#[tokio::test]
async fn tool_receipt_distinguishes_unverified_pass_and_failure() {
    let w = Workspace::new();
    for (checks, status, ok) in [
        (vec![], "unverified", true),
        (vec![check("print('ok')", true, 0)], "passed", true),
        (vec![check("raise SystemExit(1)", true, 0)], "failed", false),
    ] {
        let card = crate::hub::ClaimedCard {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            key: "test".into(),
            title: "Test".into(),
            modality: "code".into(),
            inputs: String::new(),
            acceptance: String::new(),
            deps: vec![],
            requires_internet: false,
            required_capabilities: serde_json::json!({"task":"test","workspace_path":w.0,"acceptance":checks}),
        };
        let result = crate::tools::run_code_session(
            &NoopHub,
            &w.0,
            &card,
            &Brain(true),
            chrono::Utc::now() + chrono::Duration::minutes(2),
        )
        .await
        .unwrap();
        assert_eq!(result.ok, ok);
        assert_eq!(
            result.data.as_ref().unwrap()["acceptance"]["status"],
            status
        );
        assert!(result.summary.contains(status));
    }
    let spec = CodeSessionSpec::from_required_capabilities(
        &serde_json::json!({"task":"legacy","workspace_path":w.0}),
    )
    .unwrap();
    assert!(spec.acceptance.is_empty());
}

#[tokio::test]
async fn usage_accumulates_tool_and_final_turns_and_turn_limit() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Metered(AtomicUsize);
    #[async_trait::async_trait]
    impl CodeBrain for Metered {
        async fn next_turn(
            &self,
            _: &[BrainMessage],
            _: &[ToolSpec],
        ) -> Result<BrainTurn, CodeBrainError> {
            let n = self.0.fetch_add(1, Ordering::SeqCst);
            let usage = crate::ledger::Usage {
                tokens_in: 10 + n as u64,
                tokens_out: 3,
                compute_seconds: 0.5,
            };
            Ok(if n == 0 {
                BrainTurn::ToolCalls(vec![], usage, Some("resolved-model".into()))
            } else {
                BrainTurn::Text("done".into(), usage, Some("resolved-model".into()))
            })
        }
    }
    let w = Workspace::new();
    for (limit, expected) in [(1, 10), (2, 21)] {
        let spec = CodeSessionSpec::from_required_capabilities(
            &serde_json::json!({"task":"test","workspace_path":w.0,"max_turns":limit}),
        )
        .unwrap();
        let outcome = run_session(
            &NoopHub,
            &w.0,
            Uuid::new_v4(),
            &spec,
            &Metered(AtomicUsize::new(0)),
            chrono::Utc::now() + chrono::Duration::minutes(2),
        )
        .await
        .unwrap();
        assert_eq!(outcome.model_id.as_deref(), Some("resolved-model"));
        assert_eq!(outcome.usage.tokens_in, expected);
        assert_eq!(outcome.usage.tokens_out, limit as u64 * 3);
        assert_eq!(outcome.usage.compute_seconds, limit as f64 * 0.5);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn command_tree_timeout_and_cancellation_stop_grandchildren() {
    let w = Workspace::new();
    // A grandchild would write this marker after its parent has timed out/cancelled.
    // It inherits stdout too: cleanup must close that pipe and preserve the parent's tail.
    let script = "import subprocess,sys,time,pathlib; subprocess.Popen([sys.executable,'-c',\"import time,pathlib;time.sleep(5);pathlib.Path('LEAKED').write_text('bad')\"]);pathlib.Path('READY').write_text('yes');print('READY',flush=True);time.sleep(60)";
    let result = run_command_capture(
        &w.0,
        "python3",
        &["-c".into(), script.into()],
        None,
        Duration::from_secs(2),
        4096,
        true,
    )
    .await
    .unwrap();
    assert_eq!(result["timed_out"], true);
    assert!(result["stdout"].as_str().unwrap().contains("READY"));
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(!w.0.join("LEAKED").exists());
    std::fs::remove_file(w.0.join("READY")).unwrap();
    let root = w.0.clone();
    let handle = tokio::spawn(async move {
        run_command_capture(
            &root,
            "python3",
            &["-c".into(), script.into()],
            None,
            Duration::from_secs(60),
            4096,
            true,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(15), async {
        while !w.0.join("READY").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    handle.abort();
    let _ = handle.await;
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(!w.0.join("LEAKED").exists());
}

#[tokio::test]
async fn cloud_model_identity_survives_wire_session_and_unknown_or_mixed_turns() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct WireBrain {
        turn: AtomicUsize,
        first: Option<&'static str>,
        last: Option<&'static str>,
    }
    #[async_trait::async_trait]
    impl CodeBrain for WireBrain {
        async fn next_turn(
            &self,
            _: &[BrainMessage],
            _: &[ToolSpec],
        ) -> Result<BrainTurn, CodeBrainError> {
            let n = self.turn.fetch_add(1, Ordering::SeqCst);
            let mut wire = if n == 0 {
                serde_json::json!({"type":"tool_calls","calls":[{"id":"a","name":"list_dir","arguments":{"path":"."}}],"tokens_in":5,"tokens_out":2})
            } else {
                serde_json::json!({"type":"text","text":"done","tokens_in":7,"tokens_out":3})
            };
            if let Some(model) = if n == 0 { self.first } else { self.last } {
                wire["model_id"] = model.into();
            }
            Ok(cloud_turn(serde_json::from_value(wire).unwrap()))
        }
    }
    let w = Workspace::new();
    for (first, last, expected) in [
        (Some("resolved"), Some("resolved"), Some("resolved")),
        (Some("one"), Some("two"), None),
        (None, Some("resolved"), None),
        (Some("resolved"), None, None),
        (None, None, None),
        (Some(""), Some("resolved"), None),
    ] {
        let spec = CodeSessionSpec::from_required_capabilities(&serde_json::json!({
            "task":"test", "workspace_path":w.0, "max_turns":2
        }))
        .unwrap();
        assert!(spec.model_id.is_none());
        let outcome = run_session(
            &NoopHub,
            &w.0,
            Uuid::new_v4(),
            &spec,
            &WireBrain {
                turn: AtomicUsize::new(0),
                first,
                last,
            },
            chrono::Utc::now() + chrono::Duration::minutes(2),
        )
        .await
        .unwrap();
        assert_eq!(outcome.model_id.as_deref(), expected);
        assert_eq!(outcome.usage.tokens_in, 12);
    }
    let empty = cloud_turn(
        serde_json::from_value(serde_json::json!({
            "type":"tool_calls", "calls":[], "model_id":"resolved"
        }))
        .unwrap(),
    );
    assert!(matches!(empty, BrainTurn::Text(_, _, Some(id)) if id == "resolved"));
}
