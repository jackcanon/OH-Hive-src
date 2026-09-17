//! Submission and worker contracts for explicit host acceptance checks.
use serde::{Deserialize, Serialize};

fn default_required() -> bool {
    true
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCheck {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub expect_exit: i32,
    #[serde(default = "default_required")]
    pub required: bool,
}
/// Apply the same bounds before submission and before worker execution.
pub fn validate(checks: &[AcceptanceCheck]) -> Result<(), &'static str> {
    if checks.len() > 16
        || checks.iter().any(|c| {
            c.name.trim().is_empty()
                || c.name.len() > 200
                || c.command.trim().is_empty()
                || c.command.len() > 1024
                || c.args.len() > 64
                || c.args.iter().map(String::len).sum::<usize>() > 8192
                || c.cwd.as_ref().is_some_and(|p| p.len() > 4096)
        })
    {
        return Err("acceptance checks exceed bounds or have an empty name/program");
    }
    Ok(())
}
