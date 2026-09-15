//! Owner-local managed intake. No model tool, cloud calls, or writes to source folders.
//! The owner approves the ENTIRE input for indexing, including title and source label.
//! Source adapters must persist stable IDs and monotonically increasing generations.
use super::*;

const MAX_SOURCE: usize = 1024 * 1024;
const MAX_CORPUS: i64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case", deny_unknown_fields)]
pub enum IntakeContent {
    /// Trusted source selection, never a classification returned by an agent.
    Knowledge {
        title: String,
        source_label: String,
        project: Option<String>,
        markdown: String,
    },
    /// No secret payload is accepted or persisted by this variant.
    Excluded {},
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeItem {
    pub source_id: Uuid,
    pub generation: i64,
    pub content: IntakeContent,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntakeReceipt {
    pub document_id: Uuid,
    pub revision: Option<String>,
    pub unchanged: bool,
}
struct Prepared {
    path: String,
    title: String,
    content: String,
}
fn prepare(item: &IntakeItem, id: Uuid) -> Result<Option<Prepared>> {
    let IntakeContent::Knowledge {
        title,
        source_label,
        project,
        markdown,
    } = &item.content
    else {
        return Ok(None);
    };
    check_text(title, 512)?;
    check_text(source_label, 512)?;
    check_text(markdown, MAX_SOURCE)?;
    if title.contains(['\n', '\r']) || source_label.contains(['\n', '\r']) {
        return Err(rejected("intake labels must be single-line"));
    }
    let project = project.as_deref().unwrap_or("inbox");
    if project.is_empty()
        || project.len() > 80
        || !project
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(rejected("invalid intake project"));
    }
    // Deliberately conservative: only an explicit leading document heading determines type.
    // This is a browsing label, not an assertion that the source's claims are verified.
    let heading = markdown
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let kind = match heading.trim().to_ascii_lowercase().as_str() {
        "# decision" | "# decisions" => "decisions",
        "# research" | "# benchmark" | "# test report" => "research",
        "# procedure" | "# walkthrough" => "procedures",
        _ => "notes",
    };
    // Extractive preview only: exact source substrings with line references, never invented facts.
    let mut summary = String::new();
    let mut excerpts = 0;
    let mut fenced = false;
    for (line, text) in markdown.lines().enumerate() {
        let trimmed = text.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let quote: String = trimmed.chars().take(200).collect();
        summary.push_str(&format!(
            "- Source line {}: {}{}\n",
            line + 1,
            quote,
            if quote.len() < trimmed.len() {
                " …"
            } else {
                ""
            }
        ));
        excerpts += 1;
        if excerpts == 3 {
            break;
        }
    }
    if summary.is_empty() {
        summary.push_str("No prose excerpt available; read the source below.\n");
    }
    let metadata = encode(
        &json!({"source_id":item.source_id,"generation":item.generation,
        "source_label":source_label,"source_sha256":digest(markdown),"method":"extractive-v1"}),
    )?;
    let content = format!("# {title}\n\n## Intake provenance\n{metadata}\n\n## Source excerpts (not independently verified)\n{summary}\n## Original source\n\n{markdown}");
    Ok(Some(Prepared {
        path: format!("Intake/{project}/{kind}/{id}.md"),
        title: title.clone(),
        content,
    }))
}
impl LocalHubStore {
    /// Called by a trusted host intake adapter only. No HTTP/agent write route is registered.
    /// Atomic receipt + document + FTS update. Never updates a manually edited managed document.
    /// Exclusion removes indexed text, retaining a generation tombstone (not secure disk erasure).
    pub fn vault_intake(&self, vault: Uuid, item: &IntakeItem) -> Result<IntakeReceipt> {
        if item.generation <= 0 || item.source_id.is_nil() {
            return Err(rejected("invalid intake identity"));
        }
        let hash = Sha256::digest(format!("hive-intake-v1:{vault}:{}", item.source_id).as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&hash[..16]);
        let id = Uuid::from_bytes(bytes);
        let prepared = prepare(item, id)?;
        let fingerprint = digest(&encode(item)?);
        let revision = prepared
            .as_ref()
            .map(|p| encode(&(id, &p.path, &p.title, &p.content)).map(|s| digest(&s)))
            .transpose()?;
        self.transaction(|tx| {
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM vault_sources WHERE vault_id=?1)", [vault.to_string()], |r| r.get::<_,bool>(0)).map_err(db_error)? {
                return Err(rejected("managed intake requires a separate non-folder vault"));
            }
            let old: Option<(i64,String,Option<String>)> = tx.query_row(
                "SELECT generation,fingerprint,revision FROM vault_intake WHERE vault_id=?1 AND source_id=?2",
                params![vault.to_string(),item.source_id.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(db_error)?;
            let current: Option<(String,String)> = tx.query_row("SELECT vault_id,revision FROM vault_documents WHERE id=?1",
                [id.to_string()], |r| Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
            // Protect curated edits and missing/deleted documents, including on exact retries.
            if current.as_ref().map(|(_,rev)| rev) != old.as_ref().and_then(|(_,_,rev)|rev.as_ref())
                || current.as_ref().is_some_and(|(owner,_)| owner != &vault.to_string()) {
                return Err(rejected("managed document changed outside intake; review required"));
            }
            if let Some((generation,previous,_)) = &old {
                if item.generation < *generation { return Err(rejected("stale intake generation")); }
                if item.generation == *generation {
                    if previous != &fingerprint { return Err(rejected("conflicting intake generation")); }
                    return Ok(IntakeReceipt { document_id:id, revision: old.as_ref().unwrap().2.clone(), unchanged:true });
                }
            } else {
                let count: i64 = tx.query_row("SELECT count(*) FROM vault_intake WHERE vault_id=?1", [vault.to_string()], |r|r.get(0)).map_err(db_error)?;
                if count >= 10_000 { return Err(rejected("intake source limit reached")); }
            }
            if let Some(p) = &prepared {
                let other_bytes: i64 = tx.query_row("SELECT coalesce(sum(length(CAST(content AS BLOB))),0) FROM vault_documents WHERE vault_id=?1 AND id!=?2", params![vault.to_string(),id.to_string()], |r|r.get(0)).map_err(db_error)?;
                if other_bytes + p.content.len() as i64 > MAX_CORPUS { return Err(rejected("intake corpus limit reached")); }
                tx.execute("INSERT INTO vault_documents(id,vault_id,path,revision,title,content) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET path=excluded.path,revision=excluded.revision,title=excluded.title,content=excluded.content",
                    params![id.to_string(),vault.to_string(),p.path,revision,p.title,p.content]).map_err(db_error)?;
            } else {
                tx.execute("DELETE FROM vault_documents WHERE id=?1 AND vault_id=?2", params![id.to_string(),vault.to_string()]).map_err(db_error)?;
            }
            tx.execute("INSERT INTO vault_intake(vault_id,source_id,generation,fingerprint,document_id,revision) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(vault_id,source_id) DO UPDATE SET generation=excluded.generation,fingerprint=excluded.fingerprint,revision=excluded.revision",
                params![vault.to_string(),item.source_id.to_string(),item.generation,fingerprint,id.to_string(),revision]).map_err(db_error)?;
            Ok(IntakeReceipt {document_id:id,revision:revision.clone(),unchanged:false})
        })
    }
}

#[cfg(test)]
mod tests;
