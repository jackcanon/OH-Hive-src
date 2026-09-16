//! A small trusted host adapter for ADR-028 vault intake: lets a member point at a folder of
//! Markdown, see what's there, and approve specific files one at a time into a *managed* vault
//! via [`super::vault_intake::LocalHubStore::vault_intake`] -- the "source connector" phase-1
//! intake explicitly still needs (`docs/SIF-MANAGED-VAULT-INTAKE-2026-09-14.md`: "source
//! connectors still need to submit approved source events and persist their generations").
//!
//! Deliberately thin: this module owns only file discovery/reading and generation bookkeeping.
//! Every actual intake rule (limits, edit-protection, atomicity, folder-vault rejection) lives in
//! `vault_intake` and is not duplicated here. There is no automatic or background admission of
//! anything -- approval is one explicit relative path at a time, supplied by the caller.
use super::vault::path_ok;
use super::vault_intake::{IntakeContent, IntakeItem, IntakeReceipt};
use super::*;
use cap_std::fs::Dir;
use std::io::Read;

/// Matches `vault_intake`'s own per-source Markdown limit -- checked here too so a bad candidate
/// is refused with a clear local error instead of only at the `vault_intake()` call boundary.
const MAX_FILE: u64 = 1024 * 1024;
/// Matches `vault_folder`'s own walk bound; this is advisory browsing, not authoritative state,
/// but a malicious/huge tree still shouldn't be able to make a listing call run unbounded.
const MAX_ENTRIES: usize = 20_000;

/// One `.md` file found under a candidate root, offered to a member for approval. Listing never
/// reads more than the first heading of a file and never touches the database; approving (below)
/// is the only thing that actually submits anything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntakeCandidate {
    pub relative_path: String,
    pub title: String,
    pub size: u64,
}

fn io_err(_: std::io::Error) -> HubError {
    rejected("intake source folder unavailable or unreadable")
}

fn open_root(root: &Path) -> Result<Dir> {
    if std::fs::symlink_metadata(root)
        .map_err(io_err)?
        .file_type()
        .is_symlink()
    {
        return Err(rejected("intake folder root cannot be a symlink"));
    }
    Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(io_err)
}

fn guess_title(markdown: &str, fallback: &str) -> String {
    markdown
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|s| s.chars().take(512).collect())
        .unwrap_or_else(|| fallback.to_string())
}

fn walk_candidates(
    dir: &Dir,
    prefix: &Path,
    depth: usize,
    count: &mut usize,
    out: &mut Vec<IntakeCandidate>,
) -> Result<()> {
    if depth > 32 {
        return Err(rejected("intake folder exceeds directory depth limit"));
    }
    for entry in dir.entries().map_err(io_err)? {
        let entry = entry.map_err(io_err)?;
        *count += 1;
        if *count > MAX_ENTRIES {
            return Err(rejected("intake folder exceeds entry limit"));
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue; // non-UTF-8 name: not a candidate, not a hard error for the whole listing
        };
        // Hidden entries (.git, .obsidian, .DS_Store, editor metadata) are never candidates.
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = dir.symlink_metadata(name) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue; // skip rather than fail the whole listing over one linked entry
        }
        let path = prefix.join(name);
        if meta.is_dir() {
            let child = dir.open_dir(name).map_err(io_err)?;
            walk_candidates(&child, &path, depth + 1, count, out)?;
        } else if meta.is_file() && name.ends_with(".md") {
            let relative = path
                .iter()
                .map(|c| c.to_str().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("/");
            if !path_ok(&relative) || meta.len() > MAX_FILE {
                continue; // offered as a candidate is optional; skip rather than fail the listing
            }
            let Ok(file) = dir.open(name) else { continue };
            let mut bytes = Vec::new();
            if (&file).take(MAX_FILE + 1).read_to_end(&mut bytes).is_err() {
                continue;
            }
            let title = match String::from_utf8(bytes) {
                Ok(text) => guess_title(&text, name),
                Err(_) => continue, // not UTF-8: not a candidate
            };
            out.push(IntakeCandidate {
                relative_path: relative,
                title,
                size: meta.len(),
            });
        }
    }
    Ok(())
}

/// Derives this file's stable intake source id from `(vault, relative_path)` alone -- the same
/// digest-truncation trick `vault_intake::vault_intake` already uses for its own document id, so
/// re-running this connector against the same folder always addresses the same source, with no
/// separate identity table for this module to own or lose.
fn source_id_for(vault: Uuid, relative_path: &str) -> Uuid {
    let hash = Sha256::digest(format!("hive-intake-folder-v1:{vault}:{relative_path}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    Uuid::from_bytes(bytes)
}

impl LocalHubStore {
    /// Lists `.md` files under `root` for a member to choose from. Read-only: never touches the
    /// database, never submits anything. A symlinked entry, a non-UTF-8 name, or a file over the
    /// intake size limit is simply left off the list rather than failing the whole call -- unlike
    /// `vault_folder`'s attach scan, this is advisory browsing, not the thing that defines a
    /// vault's authoritative contents. A symlinked *root* still fails outright: pointing this at
    /// a link in the first place is almost certainly a mistake worth surfacing, not silently
    /// resolving.
    pub fn vault_intake_list_candidates(
        &self,
        root: impl AsRef<Path>,
    ) -> Result<Vec<IntakeCandidate>> {
        let dir = open_root(root.as_ref())?;
        let mut out = Vec::new();
        let mut count = 0;
        walk_candidates(&dir, Path::new(""), 0, &mut count, &mut out)?;
        out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        Ok(out)
    }

    /// Submits one member-approved file into `vault` (must already be a managed, non-folder
    /// vault -- `vault_intake` itself enforces that) via [`Self::vault_intake`].
    /// `relative_path` should be one `vault_intake_list_candidates` returned for the same `root`;
    /// approval is per-file and explicit, nothing here decides what's relevant on its own.
    ///
    /// Generation bookkeeping reads this source's last-submitted `(generation, fingerprint)` back
    /// out of the existing `vault_intake` table rather than keeping any state of its own:
    /// unchanged content resubmits the same generation (`vault_intake` then reports it as an
    /// exact-retry no-op), changed content bumps it by one. A source this connector has never
    /// submitted before starts at generation 1.
    pub fn vault_intake_approve_file(
        &self,
        vault: Uuid,
        root: impl AsRef<Path>,
        relative_path: &str,
        project: Option<&str>,
    ) -> Result<IntakeReceipt> {
        if !path_ok(relative_path) || !relative_path.ends_with(".md") {
            return Err(rejected("invalid intake path"));
        }
        let dir = open_root(root.as_ref())?;
        let meta = dir.symlink_metadata(relative_path).map_err(io_err)?;
        if meta.file_type().is_symlink() {
            return Err(rejected("intake source cannot be a symlink"));
        }
        if !meta.is_file() {
            return Err(rejected("intake source must be a regular file"));
        }
        if meta.len() > MAX_FILE {
            return Err(rejected("intake source exceeds size limit"));
        }
        let file = dir.open(relative_path).map_err(io_err)?;
        let mut bytes = Vec::new();
        (&file)
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(io_err)?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(rejected("intake source exceeds size limit"));
        }
        let markdown =
            String::from_utf8(bytes).map_err(|_| rejected("intake source is not UTF-8"))?;
        let file_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
        let title = guess_title(&markdown, file_name);

        let source_id = source_id_for(vault, relative_path);
        let content = IntakeContent::Knowledge {
            title,
            source_label: format!("Folder intake: {relative_path}"),
            project: project.map(|p| p.to_string()),
            markdown,
        };

        // Read this source's own last-submitted (generation, fingerprint) back out of the
        // existing vault_intake table -- `vault_intake()` is the only writer of that table; this
        // is a plain read, not a second source of truth.
        let previous: Option<(i64, String)> = self.transaction(|tx| {
            tx.query_row(
                "SELECT generation,fingerprint FROM vault_intake WHERE vault_id=?1 AND source_id=?2",
                params![vault.to_string(), source_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_error)
        })?;

        let generation = match &previous {
            None => 1,
            Some((last_generation, last_fingerprint)) => {
                let candidate = IntakeItem {
                    source_id,
                    generation: *last_generation,
                    content: content.clone(),
                };
                let candidate_fingerprint = digest(&encode(&candidate)?);
                if &candidate_fingerprint == last_fingerprint {
                    *last_generation
                } else {
                    last_generation + 1
                }
            }
        };

        let item = IntakeItem {
            source_id,
            generation,
            content,
        };
        self.vault_intake(vault, &item)
    }
}

#[cfg(test)]
mod tests;
