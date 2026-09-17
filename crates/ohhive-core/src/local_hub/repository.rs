//! Owner-managed repository defaults. Binding is metadata, not a credential grant.
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRepository {
    pub repo_url: String,
    pub repo_ref: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepositoryProject {
    pub id: Uuid,
    pub title: String,
    pub goal: String,
    pub repository: Option<ProjectRepository>,
}

impl LocalHubStore {
    pub fn repository_projects(&self) -> Result<Vec<RepositoryProject>> {
        self.transaction(|tx| {
            let mut stmt = tx.prepare("SELECT p.id,p.title,p.goal,r.binding FROM projects p LEFT JOIN project_repositories r ON r.project_id=p.id ORDER BY p.title,p.id").map_err(db_error)?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?))).map_err(db_error)?;
            rows.map(|row| {
                let (id, title, goal, binding) = row.map_err(db_error)?;
                Ok(RepositoryProject {
                    id: Uuid::parse_str(&id).map_err(|_| rejected("invalid project identity"))?,
                    title, goal,
                    repository: binding.map(|raw| decode(&raw)).transpose()?,
                })
            }).collect()
        })
    }
    /// Trusted local administration only. Clearing a default never rewrites existing cards.
    pub fn set_project_repository(
        &self,
        project: Uuid,
        binding: Option<&ProjectRepository>,
    ) -> Result<()> {
        if let Some(binding) = binding {
            // GitHub picker bindings use canonical, credential-free HTTPS clone URLs.
            let path = binding
                .repo_url
                .strip_prefix("https://github.com/")
                .ok_or_else(|| rejected("expected a GitHub HTTPS repository URL"))?;
            let parts: Vec<_> = path.split('/').collect();
            if parts.len() != 2
                || parts.iter().any(|part| {
                    part.is_empty()
                        || *part == "."
                        || *part == ".."
                        || !part
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
                })
                || path.len() > 300
            {
                return Err(rejected("invalid GitHub repository path"));
            }
            if let Some(reference) = &binding.repo_ref {
                check_text(reference, 500)?;
                if reference.starts_with('-') || reference.chars().any(char::is_control) {
                    return Err(rejected("invalid repository reference"));
                }
            }
        }
        self.transaction(|tx| {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
                [project.to_string()], |r| r.get(0)).map_err(db_error)?;
            if !exists { return Err(rejected("project not found")); }
            match binding {
                Some(binding) => { tx.execute("INSERT INTO project_repositories VALUES(?1,?2) ON CONFLICT(project_id) DO UPDATE SET binding=excluded.binding",
                    params![project.to_string(), encode(binding)?]).map_err(db_error)?; }
                None => { tx.execute("DELETE FROM project_repositories WHERE project_id=?1", [project.to_string()]).map_err(db_error)?; }
            }
            Ok(())
        })
    }

    pub fn project_repository(&self, project: Uuid) -> Result<Option<ProjectRepository>> {
        self.transaction(|tx| read_binding(tx, project))
    }
}

fn read_binding(tx: &Transaction<'_>, project: Uuid) -> Result<Option<ProjectRepository>> {
    let raw: Option<String> = tx
        .query_row(
            "SELECT binding FROM project_repositories WHERE project_id=?1",
            [project.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    raw.map(|raw| decode(&raw)).transpose()
}

/// A child uses the parent's frozen repository, never the current project default.
/// Imported folders are not inherited: two coding tasks must not share one mutable folder.
pub(super) fn inherit_parent_repository(
    parent: &ClaimedCard,
    modality: &str,
    required: &mut Value,
) -> Result<()> {
    if modality != "code" {
        return Ok(());
    }
    let caps = required
        .as_object_mut()
        .ok_or_else(|| rejected("code capabilities must be an object"))?;
    if caps.contains_key("repo_url")
        || caps.contains_key("workspace_path")
        || parent
            .required_capabilities
            .get("workspace_path")
            .and_then(Value::as_str)
            .is_some()
    {
        return Ok(());
    }
    if let Some(repo) = parent
        .required_capabilities
        .get("repo_url")
        .and_then(Value::as_str)
    {
        caps.insert("repo_url".into(), json!(repo));
        if !caps.contains_key("repo_ref") {
            if let Some(reference) = parent
                .required_capabilities
                .get("repo_ref")
                .and_then(Value::as_str)
            {
                caps.insert("repo_ref".into(), json!(reference));
            }
        }
    }
    Ok(())
}

pub(super) fn apply_project_default(tx: &Transaction<'_>, card: &mut ClaimedCard) -> Result<()> {
    if card.modality != "code" {
        return Ok(());
    }
    let caps = card
        .required_capabilities
        .as_object_mut()
        .ok_or_else(|| rejected("code capabilities must be an object"))?;
    // Explicit task locations always win, including invalid values (validation owns those).
    if caps.contains_key("workspace_path") || caps.contains_key("repo_url") {
        return Ok(());
    }
    if let Some(binding) = read_binding(tx, card.project_id)? {
        caps.insert("repo_url".into(), json!(binding.repo_url));
        if !caps.contains_key("repo_ref") {
            if let Some(reference) = binding.repo_ref {
                caps.insert("repo_ref".into(), json!(reference));
            }
        }
    }
    Ok(())
}
