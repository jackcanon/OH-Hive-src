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
            Ok(BrainTurn::Text("Done".into()))
        } else {
            Ok(BrainTurn::ToolCalls(vec![]))
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
