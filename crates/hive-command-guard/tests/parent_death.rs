#![cfg(unix)]
#[test]
fn unix_command_ownership_regressions() {
    let result = std::process::Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/unix_guard.py"))
        .arg(env!("CARGO_BIN_EXE_hive-command-guard"))
        .status()
        .expect("Python regression fixture");
    assert!(result.success());
}
