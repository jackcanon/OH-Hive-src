//! Workspace-local SKILL.md files. Contents and metadata never grant authority or tools.
//! No prompt injection, execution, automatic learning, network or cross-machine synchronization.
use cap_std::fs::{Dir, OpenOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub const MAX_SKILL_BYTES: usize = 128 * 1024;
pub const MAX_SKILLS: usize = 256;
const MAX_HEADER: usize = 16 * 1024;
const MAX_ENTRIES: usize = 1024;
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SkillError {
    #[error("invalid skill identifier")]
    InvalidId,
    #[error("invalid skill frontmatter or procedure")]
    InvalidSkill,
    #[error("skill size or directory count limit exceeded")]
    Limit,
    #[error("skill path is not a contained regular file or directory")]
    UnsafePath,
    #[error("skill storage operation failed")]
    Io,
    #[error("skill store is busy; a prior crashed writer may require lock recovery")]
    Busy,
    #[error("skill already exists")]
    Exists,
    #[error("skill changed since it was selected")]
    Changed,
    #[error("invalid skill usage metadata")]
    InvalidUsage,
}
type Result<T> = std::result::Result<T, SkillError>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedSkill {
    pub name: String,
    pub description: String,
    pub procedure: String,
}
#[derive(Deserialize)]
struct Header {
    name: String,
    description: String,
}
/// YAML scalars (including quoted and folded descriptions) plus an unchanged Markdown body.
/// Extra metadata is tolerated for compatibility but is not returned or interpreted as authority.
pub fn parse_skill(source: &str) -> Result<ParsedSkill> {
    if source.len() > MAX_SKILL_BYTES {
        return Err(SkillError::Limit);
    }
    let text = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().ok_or(SkillError::InvalidSkill)?;
    if first.trim_end_matches(['\r', '\n']) != "---" {
        return Err(SkillError::InvalidSkill);
    }
    let start = first.len();
    let mut offset = start;
    let mut end = None;
    for line in lines {
        if offset - start > MAX_HEADER {
            return Err(SkillError::Limit);
        }
        if matches!(line.trim_end_matches(['\r', '\n']), "---" | "...") {
            end = Some((offset, offset + line.len()));
            break;
        }
        offset += line.len();
    }
    let (header_end, body_start) = end.ok_or(SkillError::InvalidSkill)?;
    if header_end - start > MAX_HEADER {
        return Err(SkillError::Limit);
    }
    let h: Header =
        serde_yaml_ng::from_str(&text[start..header_end]).map_err(|_| SkillError::InvalidSkill)?;
    let description = h
        .description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if h.name.trim().is_empty()
        || h.name.len() > 128
        || h.name.chars().any(char::is_control)
        || description.is_empty()
        || description.chars().any(char::is_control)
        || description.len() > 1024
        || text[body_start..].trim().is_empty()
    {
        return Err(SkillError::InvalidSkill);
    }
    Ok(ParsedSkill {
        name: h.name,
        description,
        procedure: text[body_start..].to_owned(),
    })
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: String,
    pub last_used_unix_ms: Option<u64>,
}
pub struct SkillDocument {
    pub summary: SkillSummary,
    pub procedure: String,
}
pub struct SkillIssue {
    pub id: String,
    pub error: SkillError,
}
pub struct SkillInventory {
    pub skills: Vec<SkillSummary>,
    pub issues: Vec<SkillIssue>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Used {
    version: u32,
    revision: String,
    unix_ms: u64,
}
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id.as_bytes()[0].is_ascii_lowercase()
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(SkillError::InvalidId);
    }
    Ok(())
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn safe_dir(parent: &Dir, name: &str, create: bool) -> Result<Dir> {
    if create {
        match parent.create_dir(name) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(SkillError::Io),
        }
    }
    let m = parent.symlink_metadata(name).map_err(|_| SkillError::Io)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(SkillError::UnsafePath);
    }
    parent.open_dir(name).map_err(|_| SkillError::UnsafePath)
}
fn regular(meta: &cap_std::fs::Metadata) -> Result<()> {
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(SkillError::UnsafePath);
    }
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        if meta.nlink() != 1 {
            return Err(SkillError::UnsafePath);
        }
    }
    Ok(())
}
fn read_bounded(dir: &Dir, name: &str, max: usize) -> Result<Vec<u8>> {
    regular(&dir.symlink_metadata(name).map_err(|_| SkillError::Io)?)?;
    let file = dir.open(name).map_err(|_| SkillError::UnsafePath)?;
    let before = file.metadata().map_err(|_| SkillError::Io)?;
    regular(&before)?;
    if before.len() > max as u64 {
        return Err(SkillError::Limit);
    }
    let mut bytes = Vec::new();
    (&file)
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SkillError::Io)?;
    let after = file.metadata().map_err(|_| SkillError::Io)?;
    if bytes.len() > max {
        return Err(SkillError::Limit);
    }
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err(SkillError::Changed);
    }
    Ok(bytes)
}
struct Lock<'a>(&'a Dir);
impl Drop for Lock<'_> {
    fn drop(&mut self) {
        let _ = self.0.remove_file(".write.lock");
    }
}
pub struct SkillStore {
    root: Dir,
}
impl SkillStore {
    /// Workspace must already exist. Creates .hive/skills, never follows a symlinked directory.
    pub fn open(workspace: impl AsRef<Path>) -> Result<Self> {
        let path = workspace.as_ref();
        let m = std::fs::symlink_metadata(path).map_err(|_| SkillError::Io)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(SkillError::UnsafePath);
        }
        let workspace = Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|_| SkillError::Io)?;
        let hive = safe_dir(&workspace, ".hive", true)?;
        Ok(Self {
            root: safe_dir(&hive, "skills", true)?,
        })
    }
    fn lock(&self) -> Result<Lock<'_>> {
        self.root
            .open_with(
                ".write.lock",
                OpenOptions::new().write(true).create_new(true),
            )
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    SkillError::Busy
                } else {
                    SkillError::Io
                }
            })?;
        Ok(Lock(&self.root))
    }
    fn ids(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for (n, e) in self.root.entries().map_err(|_| SkillError::Io)?.enumerate() {
            if n >= MAX_ENTRIES {
                return Err(SkillError::Limit);
            }
            let e = e.map_err(|_| SkillError::Io)?;
            let name = e
                .file_name()
                .into_string()
                .map_err(|_| SkillError::InvalidId)?;
            if name.starts_with('.') {
                continue;
            }
            ids.push(name);
            if ids.len() > MAX_SKILLS {
                return Err(SkillError::Limit);
            }
        }
        ids.sort();
        Ok(ids)
    }
    fn load(&self, id: &str) -> Result<SkillDocument> {
        valid_id(id)?;
        let dir = safe_dir(&self.root, id, false)?;
        let bytes = read_bounded(&dir, "SKILL.md", MAX_SKILL_BYTES)?;
        let parsed =
            parse_skill(std::str::from_utf8(&bytes).map_err(|_| SkillError::InvalidSkill)?)?;
        let revision = hash(&bytes);
        let last_used_unix_ms = match dir.symlink_metadata(".last-used.json") {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(SkillError::Io),
            Ok(_) => {
                let used: Used =
                    serde_json::from_slice(&read_bounded(&dir, ".last-used.json", 1024)?)
                        .map_err(|_| SkillError::InvalidUsage)?;
                if used.version != 1 || used.unix_ms == 0 {
                    return Err(SkillError::InvalidUsage);
                }
                if used.revision == revision {
                    Some(used.unix_ms)
                } else {
                    None
                }
            }
        };
        Ok(SkillDocument {
            summary: SkillSummary {
                id: id.to_owned(),
                name: parsed.name,
                description: parsed.description,
                revision,
                last_used_unix_ms,
            },
            procedure: parsed.procedure,
        })
    }
    /// Returns compact metadata only. Malformed entries are explicit issues, not silent omissions.
    pub fn list(&self) -> Result<SkillInventory> {
        let _lock = self.lock()?;
        let mut out = SkillInventory {
            skills: Vec::new(),
            issues: Vec::new(),
        };
        for id in self.ids()? {
            match self.load(&id) {
                Ok(doc) => out.skills.push(doc.summary),
                Err(error) => out.issues.push(SkillIssue { id, error }),
            }
        }
        Ok(out)
    }
    pub fn read(&self, id: &str) -> Result<SkillDocument> {
        let _lock = self.lock()?;
        self.load(id)
    }
    /// Create-only. Existing procedures are never overwritten by this API.
    pub fn write_new(&self, id: &str, source: &str) -> Result<SkillSummary> {
        valid_id(id)?;
        parse_skill(source)?;
        let _lock = self.lock()?;
        if self.ids()?.len() >= MAX_SKILLS {
            return Err(SkillError::Limit);
        }
        let dir = safe_dir(&self.root, id, true)?;
        match dir.symlink_metadata("SKILL.md") {
            Ok(_) => return Err(SkillError::Exists),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(SkillError::Io),
        }
        match dir.symlink_metadata(".last-used.json") {
            Ok(_) => return Err(SkillError::InvalidUsage),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(SkillError::Io),
        }
        atomic_write(&dir, "SKILL.md", source.as_bytes(), true)?;
        Ok(self.load(id)?.summary)
    }
    /// Call when the procedure is actually used, not when inventory or previews are read.
    pub fn mark_used(&self, id: &str, revision: &str) -> Result<()> {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SkillError::InvalidUsage)?
            .as_millis();
        self.mark_used_at(
            id,
            revision,
            u64::try_from(ms).map_err(|_| SkillError::InvalidUsage)?,
        )
    }
    fn mark_used_at(&self, id: &str, revision: &str, unix_ms: u64) -> Result<()> {
        let _lock = self.lock()?;
        let doc = self.load(id)?;
        if doc.summary.revision != revision {
            return Err(SkillError::Changed);
        }
        if unix_ms == 0 {
            return Err(SkillError::InvalidUsage);
        }
        let dir = safe_dir(&self.root, id, false)?;
        let bytes = serde_json::to_vec(&Used {
            version: 1,
            revision: revision.to_owned(),
            unix_ms: unix_ms.max(doc.summary.last_used_unix_ms.unwrap_or(0)),
        })
        .map_err(|_| SkillError::InvalidUsage)?;
        atomic_write(&dir, ".last-used.json", &bytes, false)
    }
}
fn atomic_write(dir: &Dir, name: &str, bytes: &[u8], create_only: bool) -> Result<()> {
    let temp = format!(".tmp-{}", Uuid::new_v4());
    let result = (|| {
        let mut file = dir
            .open_with(&temp, OpenOptions::new().write(true).create_new(true))
            .map_err(|_| SkillError::Io)?;
        file.write_all(bytes).map_err(|_| SkillError::Io)?;
        file.sync_all().map_err(|_| SkillError::Io)?;
        drop(file);
        if create_only {
            dir.hard_link(&temp, dir, name).map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    SkillError::Exists
                } else {
                    SkillError::Io
                }
            })?;
        } else {
            dir.rename(&temp, dir, name).map_err(|_| SkillError::Io)?;
        }
        Ok(())
    })();
    let _ = dir.remove_file(&temp);
    result
}
#[cfg(test)]
mod tests;
