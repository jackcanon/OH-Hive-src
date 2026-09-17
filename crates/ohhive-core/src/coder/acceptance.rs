//! Explicit host-declared acceptance commands; never inferred from workspace files.
use super::*;
pub const ACCEPTANCE_TIMEOUT: Duration = Duration::from_secs(900);
const TAIL_BYTES: usize = 4096;
pub use crate::acceptance::AcceptanceCheck;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcceptanceResult {
    pub name: String,
    pub command_line: String,
    pub exit_status: Option<i32>,
    pub passed: bool,
    pub required: bool,
    pub timed_out: bool,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "results", rename_all = "snake_case")]
pub enum AcceptanceOutcome {
    Unverified,
    Skipped,
    Passed(Vec<AcceptanceResult>),
    Failed(Vec<AcceptanceResult>),
    Errored(Vec<AcceptanceResult>, String),
}
impl AcceptanceOutcome {
    pub fn blocks_completion(&self) -> bool {
        matches!(self, Self::Failed(_) | Self::Errored(..))
    }
    pub fn receipt(&self) -> String {
        format!(
            "Acceptance checks: {}",
            serde_json::to_string(self).expect("serializable acceptance outcome")
        )
    }
}
pub(super) fn validate(checks: &[AcceptanceCheck]) -> Result<(), CoderError> {
    crate::acceptance::validate(checks).map_err(|message| CoderError::InvalidSpec(message.into()))
}
pub(super) fn command_line(c: &AcceptanceCheck) -> String {
    std::iter::once(&c.command)
        .chain(c.args.iter())
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(" ")
}
pub(super) fn prompt(checks: &[AcceptanceCheck]) -> String {
    if checks.is_empty() {
        return String::new();
    }
    let mut text=String::from("\n\nDeclared acceptance checks (required checks must pass before completion). Run them yourself before finishing; report failures plainly. Commands run directly, without a shell:\n");
    for c in checks {
        text.push_str(&format!(
            "- {}: {} (cwd {:?}, expected exit {}, {})\n",
            c.name,
            command_line(c),
            c.cwd.as_deref().unwrap_or("."),
            c.expect_exit,
            if c.required { "required" } else { "advisory" }
        ));
    }
    text
}
pub(super) async fn run(
    hub: &dyn Hub,
    card: Uuid,
    root: &Path,
    checks: &[AcceptanceCheck],
    lease: chrono::DateTime<chrono::Utc>,
    timeout: Duration,
) -> AcceptanceOutcome {
    if checks.is_empty() {
        return AcceptanceOutcome::Unverified;
    }
    let mut results = Vec::new();
    let mut error = None;
    for c in checks {
        if chrono::Utc::now() >= lease {
            return AcceptanceOutcome::Skipped;
        }
        post_event(
            hub,
            "acceptance_check",
            &format!("Running acceptance check: {}", c.name),
            serde_json::json!({"card_id":card,"name":c.name,"command":c.command,"args":c.args}),
        )
        .await;
        let remaining = (lease - chrono::Utc::now()).to_std().unwrap_or_default();
        if remaining.is_zero() {
            return AcceptanceOutcome::Skipped;
        }
        let mut result = AcceptanceResult {
            name: c.name.clone(),
            command_line: command_line(c),
            exit_status: None,
            passed: false,
            required: c.required,
            timed_out: false,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: None,
        };
        match run_command_capture(
            root,
            &c.command,
            &c.args,
            c.cwd.as_deref(),
            timeout.min(remaining),
            TAIL_BYTES,
            true,
        )
        .await
        {
            Ok(v) => {
                result.exit_status = v["exit_code"].as_i64().map(|n| n as i32);
                result.timed_out = v["timed_out"].as_bool().unwrap_or(false);
                result.passed = !result.timed_out && result.exit_status == Some(c.expect_exit);
                result.stdout_tail = v["stdout"].as_str().unwrap_or_default().into();
                result.stderr_tail = v["stderr"].as_str().unwrap_or_default().into();
            }
            Err(e) => {
                let message = truncate_preview(&e.to_string(), 1000);
                result.error = Some(message.clone());
                if c.required {
                    error = Some(format!("{}: {message}", c.name));
                }
            }
        }
        post_event(
            hub,
            "acceptance_result",
            &format!(
                "Acceptance check {}: {}",
                c.name,
                if result.passed {
                    "passed"
                } else {
                    "did not pass"
                }
            ),
            serde_json::json!({"card_id":card,"result":result}),
        )
        .await;
        results.push(result);
    }
    if let Some(e) = error {
        AcceptanceOutcome::Errored(results, e)
    } else if results.iter().any(|r| r.required && !r.passed) {
        AcceptanceOutcome::Failed(results)
    } else {
        AcceptanceOutcome::Passed(results)
    }
}
