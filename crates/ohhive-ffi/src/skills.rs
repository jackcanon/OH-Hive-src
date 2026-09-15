//! FFI wrapper for the workspace-local Skills library (ADR-027 decision 5's "Settings surface,
//! not silent background state"). `hive_core::skills::SkillStore` already exists and is wired
//! into `coder.rs` (mine, self-improving-skill-agent), but until now nothing exposed it to Swift
//! -- a member had no way to see what a card's agent had saved to `.hive/skills/`, let alone
//! delete a bad one, which decision 5 calls "the load-bearing mitigation" for skills being
//! written fully automatically with no approval step.
//!
//! Stateless and synchronous, same reasoning as `local_hub.rs`'s vault methods: `SkillStore` is
//! plain local-disk I/O (`cap_std`), no network, so these are plain `pub fn`, not `async fn`, and
//! `HiveNode` holds no skills-specific state at all -- every call just opens the store fresh at
//! the given workspace path. Skills are workspace-local by design (ADR-027), so unlike the vault
//! there's no single "the" library to browse: the caller supplies which workspace (a local folder
//! containing, or destined to contain, `.hive/skills/`) each time.

use crate::{HiveError, HiveNode};
use hive_core::skills::{
    SkillDocument as CoreDocument, SkillInventory as CoreInventory, SkillIssue as CoreIssue,
    SkillStore, SkillSummary as CoreSummary,
};

#[derive(uniffi::Record, Clone)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: String,
    pub last_used_unix_ms: Option<u64>,
}
impl From<CoreSummary> for SkillSummary {
    fn from(s: CoreSummary) -> Self {
        Self {
            id: s.id,
            name: s.name,
            description: s.description,
            revision: s.revision,
            last_used_unix_ms: s.last_used_unix_ms,
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct SkillDocument {
    pub summary: SkillSummary,
    pub procedure: String,
}
impl From<CoreDocument> for SkillDocument {
    fn from(d: CoreDocument) -> Self {
        Self {
            summary: d.summary.into(),
            procedure: d.procedure,
        }
    }
}

/// A `.hive/skills/<id>/` entry that failed to load -- malformed frontmatter, oversized, usage
/// metadata that doesn't parse. Reported, never silently dropped, mirroring `SkillStore::list`'s
/// own contract: a broken skill costs its own visibility, not the rest of the library's.
#[derive(uniffi::Record, Clone)]
pub struct SkillIssue {
    pub id: String,
    pub error: String,
}
impl From<CoreIssue> for SkillIssue {
    fn from(i: CoreIssue) -> Self {
        Self {
            id: i.id,
            error: i.error.to_string(),
        }
    }
}

#[derive(uniffi::Record, Clone)]
pub struct SkillInventory {
    pub skills: Vec<SkillSummary>,
    pub issues: Vec<SkillIssue>,
}
impl From<CoreInventory> for SkillInventory {
    fn from(inv: CoreInventory) -> Self {
        Self {
            skills: inv.skills.into_iter().map(Into::into).collect(),
            issues: inv.issues.into_iter().map(Into::into).collect(),
        }
    }
}

#[uniffi::export]
impl HiveNode {
    /// Lists the skills saved under `<workspace_path>/.hive/skills/`. An empty inventory (no
    /// error) covers both "no skills yet" and "not a workspace with a `.hive/` at all" -- same
    /// as `coder.rs`'s own `skills_prompt_block`, since neither is a Settings-surface failure.
    pub fn skills_list(&self, workspace_path: String) -> Result<SkillInventory, HiveError> {
        Ok(SkillStore::open(&workspace_path)
            .map_err(HiveError::from)?
            .list()
            .map_err(HiveError::from)?
            .into())
    }

    /// Reads one skill's full procedure text, so a Settings screen can show it before the member
    /// decides whether to delete it.
    pub fn skills_read(
        &self,
        workspace_path: String,
        id: String,
    ) -> Result<SkillDocument, HiveError> {
        Ok(SkillStore::open(&workspace_path)
            .map_err(HiveError::from)?
            .read(&id)
            .map_err(HiveError::from)?
            .into())
    }

    /// Deletes one skill. `revision` must match what `skills_list`/`skills_read` last showed the
    /// caller, so a Settings screen holding a stale list can't delete a skill out from under a
    /// state it never actually saw (id reused, already deleted by another call).
    pub fn skills_delete(
        &self,
        workspace_path: String,
        id: String,
        revision: String,
    ) -> Result<(), HiveError> {
        SkillStore::open(&workspace_path)
            .map_err(HiveError::from)?
            .delete(&id, &revision)
            .map_err(HiveError::from)
    }
}
