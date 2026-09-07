//! End-to-end sandbox test against a real compiled WASI Preview 2 component
//! (`tests/fixtures/sandbox-echo.wasm`, see `tests/fixtures/README.md`).
//!
//! `src/sandbox.rs`'s own `#[cfg(test)]` tests only exercise the
//! policy/enforcement layer (tools_level refusal, the net-policy AND gate) —
//! none of them ever load or run an actual component. This test is the one
//! place that does: it proves the whole path — engine construction, WASI
//! context + scratch-dir preopen, fuel/memory limits, component
//! instantiation, `wasi:cli/run` — actually executes a real guest and that
//! the guest's filesystem writes land where they should (and only there).

#![cfg(feature = "sandbox")]

use ohhive_core::capability::ToolsLevel;
use ohhive_core::{NetPolicy, Sandbox, SandboxLimits};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sandbox-echo.wasm")
}

fn inputs_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sandbox-inputs-echo.wasm")
}

fn scratch_for(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ohhive-sandbox-e2e-{name}"))
}

#[tokio::test]
async fn runs_a_real_component_and_its_filesystem_write_lands_in_scratch() {
    let sandbox = Sandbox::new().expect("engine construction");
    let scratch = scratch_for("happy-path");

    sandbox
        .run(
            &fixture(),
            &scratch,
            None,
            ToolsLevel::SandboxedTools,
            NetPolicy::closed(), // this fixture never touches the network — prove that's fine
            SandboxLimits::default(),
            "e2e-happy-path",
        )
        .await
        .expect("a trivial, well-behaved component should run to completion");

    let output = scratch.join("sandbox-echo-output.txt");
    let content = std::fs::read_to_string(&output)
        .unwrap_or_else(|e| panic!("guest should have written {}: {e}", output.display()));
    assert_eq!(content, "hello from the sandbox\n");

    let _ = std::fs::remove_dir_all(&scratch);
}

#[tokio::test]
async fn refuses_the_same_component_when_tools_level_is_inference_only() {
    // The exact D48 guarantee, but now against a real component path (not
    // `/nonexistent/...` like the unit test in src/sandbox.rs) — confirms the
    // refusal really does happen before the component is ever touched, for a
    // component that would otherwise run just fine.
    let sandbox = Sandbox::new().expect("engine construction");
    let scratch = scratch_for("inference-only-refusal");

    let err = sandbox
        .run(
            &fixture(),
            &scratch,
            None,
            ToolsLevel::InferenceOnly,
            NetPolicy::new(true, true),
            SandboxLimits::default(),
            "e2e-refusal",
        )
        .await
        .expect_err("inference_only must refuse even a perfectly valid component");

    assert!(matches!(err, ohhive_core::SandboxError::ToolsDisabled));
    assert!(
        !scratch.exists(),
        "refusing before touching wasmtime means scratch is never created"
    );
}

#[tokio::test]
async fn a_starved_fuel_budget_traps_instead_of_running_forever() {
    // Not zero (instantiation itself burns some fuel) but far too little for
    // this component — a real std binary doing filesystem I/O — to finish.
    let sandbox = Sandbox::new().expect("engine construction");
    let scratch = scratch_for("fuel-starved");
    let tiny = SandboxLimits {
        fuel: 1000,
        ..SandboxLimits::default()
    };

    let err = sandbox
        .run(
            &fixture(),
            &scratch,
            None,
            ToolsLevel::SandboxedTools,
            NetPolicy::closed(),
            tiny,
            "e2e-fuel-starved",
        )
        .await
        .expect_err("a component starved of fuel should trap, not silently succeed");

    // Which variant depends on *when* fuel runs out: a budget this tiny is usually
    // exhausted during component instantiation (before `wasi:cli/run` is even
    // called), which this crate reports as `Instantiate` rather than `Trapped` —
    // both are the same underlying wasmtime fuel-exhaustion trap, just caught at
    // different call sites in `Sandbox::run`. Either is the fuel limit doing its job.
    assert!(
        matches!(
            err,
            ohhive_core::SandboxError::Trapped(_) | ohhive_core::SandboxError::Instantiate(_)
        ),
        "expected a fuel-exhaustion error, got: {err:?}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

#[tokio::test]
async fn inputs_dir_is_readable_at_in_and_not_writable() {
    let sandbox = Sandbox::new().expect("engine construction");
    let scratch = scratch_for("inputs-dir");
    let inputs = std::env::temp_dir().join("ohhive-sandbox-e2e-inputs-dir-staged");
    std::fs::create_dir_all(&inputs).expect("create staged inputs dir");
    std::fs::write(inputs.join("hello.txt"), "staged by artifact_get\n")
        .expect("stage the input file the fixture expects");

    sandbox
        .run(
            &inputs_fixture(),
            &scratch,
            Some(&inputs),
            ToolsLevel::SandboxedTools,
            NetPolicy::closed(),
            SandboxLimits::default(),
            "e2e-inputs-dir",
        )
        .await
        .expect("a component that only reads /in and scratch should run to completion");

    let copied = std::fs::read_to_string(scratch.join("from-in.txt"))
        .expect("guest should have copied the staged file into scratch");
    assert_eq!(copied, "staged by artifact_get\n");

    let verdict = std::fs::read_to_string(scratch.join("write-attempt-result.txt"))
        .expect("guest should have recorded whether the /in write was rejected");
    assert_eq!(verdict, "ok: write to /in was rejected");

    // The read-only mount itself is untouched — this is the same staged dir a
    // real artifact_get would have populated once; a second exec_wasm call
    // reusing it must see it unmodified.
    assert!(!inputs.join("should-fail.txt").exists());

    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_dir_all(&inputs);
}
