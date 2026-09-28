//! Private fleet vault storage. Administration is host-local; readers need an explicit grant.
//! Document identity survives path/content changes. Reads require the current indexed revision.
use super::*;
/// Host-local administration inventory. No reader/HTTP method exposes these grants.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultComputerAccess {
    pub node_id: Uuid,
    pub name: String,
    pub allowed: bool,
    pub active: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultInfo {
    pub id: Uuid,
    pub name: String,
    pub state: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultDocument {
    pub id: Uuid,
    pub vault_id: Uuid,
    pub path: String,
    pub revision: String,
    pub title: String,
    pub content: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultHit {
    pub id: Uuid,
    pub path: String,
    pub revision: String,
    pub title: String,
    pub snippet: String,
    pub score: f64,
    /// Unix seconds, best-effort. See `extract_document_date` -- most documents have neither
    /// convention it looks for and this is simply `None`, which the GUI shows as no date badge.
    pub document_date: Option<i64>,
}
/// Best-effort document date, parsed once at write time from the document's own content so no
/// caller (CLI `vault put`, the note editor, Spark's importer, folder intake) needs a separate
/// date parameter or format change. Recognizes two conventions already in use:
/// - YAML frontmatter `date: ...` inside a leading `---`/`---` fence (the convention already
///   recommended for Library intake generally).
/// - Spark's raw meeting export, a bare `Date: YYYY-MM-DD HH:MM` line with no frontmatter, within
///   the first 20 lines (see `SparkMeetingImporter.swift`'s `SparkMeetingFormat.markdown`).
/// Never fails the write it's called from -- an unparsed or absent date just means no date badge,
/// not a rejected document.
pub fn extract_document_date(content: &str) -> Option<i64> {
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some(v) = line.trim().strip_prefix("date:") {
                    if let Some(ts) = parse_date_value(v) {
                        return Some(ts);
                    }
                }
            }
        }
    }
    content
        .lines()
        .take(20)
        .find_map(|line| line.trim().strip_prefix("Date:").and_then(parse_date_value))
}
fn parse_date_value(raw: &str) -> Option<i64> {
    let v = raw.trim().trim_matches('"').trim_matches('\'');
    if v.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(v) {
        return Some(dt.timestamp());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(v, "%Y-%m-%d %H:%M") {
        return Some(ndt.and_utc().timestamp());
    }
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d") {
        return nd.and_hms_opt(0, 0, 0).map(|ndt| ndt.and_utc().timestamp());
    }
    None
}
pub(super) fn path_ok(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains(['\\', '\0', ':'])
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
        && path.ends_with(".md")
}
impl LocalHubStore {
    /// Re-publishes every hand-curated vault (no row in `vault_sources`) as ready after a store
    /// reopen. `from_connection` marks *every* vault unavailable on open so a folder-backed vault
    /// never shows stale "ready" content before its watcher re-scans -- but a hand-curated vault
    /// has no watcher to ever undo that, so without this it stays permanently unavailable after
    /// the very first restart following its creation. Folder-backed vaults are deliberately
    /// excluded here; their own reconciliation path is what's allowed to mark them ready again.
    pub fn vault_reopen_manual(&self) -> Result<()> {
        self.transaction(|tx| {
            tx.execute(
                "UPDATE vaults SET state='ready' WHERE state='unavailable' \
                 AND id NOT IN (SELECT vault_id FROM vault_sources)",
                [],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }
    pub fn vault_create(&self, name: &str) -> Result<Uuid> {
        check_text(name, 200)?;
        let id = Uuid::new_v4();
        self.transaction(|tx| {
            tx.execute(
                "INSERT INTO vaults(id,name) VALUES(?1,?2)",
                params![id.to_string(), name],
            )
            .map_err(db_error)?;
            Ok(id)
        })
    }
    /// Grants every currently-paired, non-revoked machine read access to every existing
    /// collection -- the backfill for a machine that paired after some collections already
    /// existed (new collections get this automatically from `vault_create`). Returns the
    /// number of (collection, machine) grants actually added; already-granted pairs are
    /// left alone. Safe and idempotent to re-run, including on a schedule.
    pub fn vault_reconcile_grants(&self) -> Result<u64> {
        self.transaction(|tx| {
            let n = tx
                .execute(
                    "INSERT OR IGNORE INTO vault_readers(vault_id, node_id) \
                     SELECT v.id, n.id FROM vaults v CROSS JOIN nodes n \
                     WHERE EXISTS(SELECT 1 FROM local_node_keys k WHERE k.node_id = n.id AND k.revoked = 0)",
                    [],
                )
                .map_err(db_error)?;
            Ok(n as u64)
        })
    }
    /// Renames a collection in place. Existing documents, grants, and history are untouched --
    /// only the display name changes. GUI right-click "Rename..." (Jack, 2026-09-27: "you
    /// have to right click on the collection title and select Delete/Rename from a drop down
    /// menu").
    pub fn vault_rename(&self, vault: Uuid, name: &str) -> Result<()> {
        check_text(name, 200)?;
        self.transaction(|tx| {
            let n = tx
                .execute(
                    "UPDATE vaults SET name=?1 WHERE id=?2",
                    params![name, vault.to_string()],
                )
                .map_err(db_error)?;
            if n == 0 {
                return Err(rejected("collection not found"));
            }
            Ok(())
        })
    }
    /// Permanently deletes a collection and its live catalog state: every document (removed the
    /// same way `vault_remove_document` does, so FTS and the observation/provenance audit trail
    /// stay consistent), every reader grant, its folder-source linkage, its intake receipts, and
    /// its maintenance/retention schedule -- none of that means anything once the collection is
    /// gone. Deliberately does NOT delete `vault_observations`, `vault_provenance`,
    /// `vault_curation_events`, or `vault_archives`: this codebase's own convention for the
    /// curation overlay is "never moves or deletes source files," and `vault_archives` in
    /// particular holds real document snapshots -- keeping them orphaned-but-intact is a safety
    /// net if a collection is deleted by mistake, at the cost of leaving some historical rows
    /// with no live vault to point at. GUI right-click "Delete" (behind a confirmation dialog in
    /// the UI, not enforced here -- this call is unconditional once made).
    pub fn vault_delete(&self, vault: Uuid) -> Result<()> {
        self.transaction(|tx| {
            let exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM vaults WHERE id=?1)",
                    [vault.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if !exists {
                return Err(rejected("collection not found"));
            }
            tx.execute(
                "DELETE FROM vault_documents WHERE vault_id=?1",
                [vault.to_string()],
            )
            .map_err(db_error)?;
            tx.execute(
                "DELETE FROM vault_readers WHERE vault_id=?1",
                [vault.to_string()],
            )
            .map_err(db_error)?;
            tx.execute(
                "DELETE FROM vault_sources WHERE vault_id=?1",
                [vault.to_string()],
            )
            .map_err(db_error)?;
            tx.execute(
                "DELETE FROM vault_intake WHERE vault_id=?1",
                [vault.to_string()],
            )
            .map_err(db_error)?;
            tx.execute(
                "DELETE FROM vault_maintenance WHERE vault_id=?1",
                [vault.to_string()],
            )
            .map_err(db_error)?;
            tx.execute("DELETE FROM vaults WHERE id=?1", [vault.to_string()])
                .map_err(db_error)?;
            Ok(())
        })
    }
    pub fn vault_grant(&self, vault: Uuid, node: Uuid, enabled: bool) -> Result<()> {
        self.transaction(|tx| {
            if enabled {
                tx.execute(
                    "INSERT OR IGNORE INTO vault_readers VALUES(?1,?2)",
                    params![vault.to_string(), node.to_string()],
                )
            } else {
                tx.execute(
                    "DELETE FROM vault_readers WHERE vault_id=?1 AND node_id=?2",
                    params![vault.to_string(), node.to_string()],
                )
            }
            .map_err(db_error)?;
            Ok(())
        })
    }
    /// Includes revoked computers with saved grants so the owner can remove stale access.
    pub fn vault_computer_access(&self, vault: Uuid) -> Result<Vec<VaultComputerAccess>> {
        self.transaction(|tx| {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM vaults WHERE id=?1)", [vault.to_string()], |r| r.get(0)).map_err(db_error)?;
            if !exists { return Err(rejected("collection not found")); }
            let mut q = tx.prepare("SELECT n.id,n.name,EXISTS(SELECT 1 FROM vault_readers r WHERE r.vault_id=?1 AND r.node_id=n.id),EXISTS(SELECT 1 FROM local_node_keys k WHERE k.node_id=n.id AND k.revoked=0) FROM nodes n WHERE EXISTS(SELECT 1 FROM local_node_keys k WHERE k.node_id=n.id) ORDER BY n.name,n.id").map_err(db_error)?;
            let rows = q.query_map([vault.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?, r.get::<_, bool>(3)?))).map_err(db_error)?;
            rows.map(|row| {
                let (id, name, allowed, active) = row.map_err(db_error)?;
                Ok(VaultComputerAccess { node_id: Uuid::parse_str(&id).map_err(|_| rejected("invalid stored computer identity"))?, name, allowed, active })
            }).collect()
        })
    }
    /// Local owner UI only. Pairing and agent role selection do not call this method.
    pub fn vault_set_computer_access(&self, vault: Uuid, node: Uuid, allowed: bool) -> Result<()> {
        self.transaction(|tx| {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM vaults WHERE id=?1)", [vault.to_string()], |r| r.get(0)).map_err(db_error)?;
            if !exists { return Err(rejected("collection not found")); }
            let (known, active): (bool, bool) = tx.query_row("SELECT EXISTS(SELECT 1 FROM local_node_keys WHERE node_id=?1),EXISTS(SELECT 1 FROM local_node_keys WHERE node_id=?1 AND revoked=0)", [node.to_string()], |r| Ok((r.get(0)?, r.get(1)?))).map_err(db_error)?;
            if !known || (allowed && !active) { return Err(rejected("computer is not currently paired")); }
            if allowed {
                tx.execute("INSERT OR IGNORE INTO vault_readers(vault_id,node_id) VALUES(?1,?2)", params![vault.to_string(),node.to_string()]).map_err(db_error)?;
            } else {
                tx.execute("DELETE FROM vault_readers WHERE vault_id=?1 AND node_id=?2", params![vault.to_string(),node.to_string()]).map_err(db_error)?;
            }
            Ok(())
        })
    }
    /// Indexer supplies complete, bounded batches and marks ready only after reconciliation.
    pub fn vault_set_available(&self, vault: Uuid, available: bool) -> Result<()> {
        self.transaction(|tx| {
            let n = tx
                .execute(
                    "UPDATE vaults SET state=?2 WHERE id=?1",
                    params![
                        vault.to_string(),
                        if available { "ready" } else { "unavailable" }
                    ],
                )
                .map_err(db_error)?;
            if n != 1 {
                return Err(rejected("vault not found"));
            }
            Ok(())
        })
    }
    /// Atomic upsert of an explicit identity. Never infers identity from identical contents.
    pub fn vault_put(
        &self,
        vault: Uuid,
        id: Uuid,
        path: &str,
        title: &str,
        content: &str,
    ) -> Result<String> {
        if !path_ok(path) || title.len() > 4096 || content.len() > 4 * 1024 * 1024 {
            return Err(rejected("invalid vault document"));
        }
        let revision = digest(&encode(&(id, path, title, content))?);
        let document_date = extract_document_date(content);
        self.transaction(|tx|{
   if tx.query_row("SELECT EXISTS(SELECT 1 FROM vault_sources WHERE vault_id=?1)",[vault.to_string()],|r|r.get::<_,bool>(0)).map_err(db_error)? {return Err(rejected("folder vault writes require reconciliation"));}
   let owner:Option<String>=tx.query_row("SELECT vault_id FROM vault_documents WHERE id=?1",[id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
   if owner.as_deref().is_some_and(|v|v!=vault.to_string()){return Err(rejected("document belongs to another vault"))}
   tx.execute("INSERT INTO vault_documents(id,vault_id,path,revision,title,content,document_date) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET path=excluded.path,revision=excluded.revision,title=excluded.title,content=excluded.content,document_date=excluded.document_date",params![id.to_string(),vault.to_string(),path,revision,title,content,document_date]).map_err(db_error)?;
   Ok(revision)
  })
    }
    /// Owner-only inventory across every vault this store holds, regardless of reader grants --
    /// for the desktop app's own "manage my vaults" screen on the host machine. Never exposed
    /// over HTTP (nothing outside `LocalHubStore`'s direct owner surface is).
    pub fn vault_list_all(&self) -> Result<Vec<VaultInfo>> {
        self.transaction(|tx| {
            let mut q = tx
                .prepare("SELECT id,name,state FROM vaults ORDER BY name,id LIMIT 1000")
                .map_err(db_error)?;
            let rows = q
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(db_error)?;
            rows.map(|r| {
                let (id, name, state) = r.map_err(db_error)?;
                Ok(VaultInfo {
                    id: id.parse().map_err(|_| rejected("invalid vault identity"))?,
                    name,
                    state,
                })
            })
            .collect()
        })
    }
    /// Host-local counterpart of `LocalHub::vault_search` -- for the CLI (`hive hub vault
    /// search`), which runs directly against this machine's own store file and has no node
    /// identity to check a reader grant against (see the module doc: administration here is
    /// host-local by construction). Same query/ranking as the node-scoped version, minus the
    /// `vault_access` reader-grant check.
    pub fn vault_search_local(
        &self,
        vault: Uuid,
        query: &str,
        limit: u32,
    ) -> Result<Vec<VaultHit>> {
        check_text(query, 2048)?;
        if !(1..=100).contains(&limit) {
            return Err(rejected("search limit must be 1..100"));
        }
        let terms: Vec<_> = query
            .split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        let query = terms.join(" AND ");
        self.transaction(|tx| {
            let mut q = tx.prepare("SELECT d.id,d.path,d.revision,d.title,snippet(vault_fts,1,'','', ' … ',32),bm25(vault_fts),d.document_date FROM vault_fts JOIN vault_documents d ON d.rowid=vault_fts.rowid WHERE vault_fts MATCH ?1 AND d.vault_id=?2 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=d.id AND a.vault_id=d.vault_id) ORDER BY bm25(vault_fts),d.id LIMIT ?3").map_err(db_error)?;
            let rows = q.query_map(params![query, vault.to_string(), limit], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get::<_, Option<i64>>(6)?))).map_err(db_error)?;
            rows.map(|r| {
                let (id, path, revision, title, snippet, score, document_date) = r.map_err(db_error)?;
                Ok(VaultHit { id: id.parse().map_err(|_| rejected("invalid document identity"))?, path, revision, title, snippet, score, document_date })
            }).collect()
        })
    }
    /// Host-local counterpart of `LocalHub::vault_list_documents` -- see `vault_search_local`
    /// for why this exists alongside the node-scoped version instead of reusing it.
    pub fn vault_list_documents_local(&self, vault: Uuid, limit: u32) -> Result<Vec<VaultHit>> {
        if !(1..=1000).contains(&limit) {
            return Err(rejected("list limit must be 1..1000"));
        }
        self.transaction(|tx| {
            let mut q = tx
                .prepare(
                    "SELECT id,path,revision,title,content,document_date FROM vault_documents WHERE vault_id=?1 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=vault_documents.id AND a.vault_id=vault_documents.vault_id) ORDER BY title,path LIMIT ?2",
                )
                .map_err(db_error)?;
            let rows = q
                .query_map(params![vault.to_string(), limit], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<i64>>(5)?))
                })
                .map_err(db_error)?;
            rows.map(|r| {
                let (id, path, revision, title, content, document_date) = r.map_err(db_error)?;
                let snippet: String = content.chars().take(96).collect();
                Ok(VaultHit { id: id.parse().map_err(|_| rejected("invalid document identity"))?, path, revision, title, snippet, score: 0.0, document_date })
            }).collect()
        })
    }
    pub fn vault_remove_document(&self, vault: Uuid, id: Uuid) -> Result<()> {
        self.transaction(|tx| {
            if tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM vault_sources WHERE vault_id=?1)",
                    [vault.to_string()],
                    |r| r.get::<_, bool>(0),
                )
                .map_err(db_error)?
            {
                return Err(rejected("folder vault writes require reconciliation"));
            }
            tx.execute(
                "DELETE FROM vault_documents WHERE id=?1 AND vault_id=?2",
                params![id.to_string(), vault.to_string()],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }
}
impl LocalHub {
    pub(super) fn vault_access(
        &self,
        tx: &Transaction<'_>,
        node: &str,
        vault: Uuid,
        require_ready: bool,
    ) -> Result<VaultInfo> {
        let row:Option<(String,String)>=tx.query_row("SELECT name,state FROM vaults v JOIN vault_readers r ON r.vault_id=v.id WHERE v.id=?1 AND r.node_id=?2",params![vault.to_string(),node],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
        let (name, state) = row.ok_or_else(|| rejected("vault not available to this node"))?;
        if require_ready && state != "ready" {
            return Err(HubError::Transport("vault unavailable".into()));
        }
        Ok(VaultInfo {
            id: vault,
            name,
            state,
        })
    }
    pub fn vault_list(&self) -> Result<Vec<VaultInfo>> {
        self.with_node(|tx,node|{
   let mut q=tx.prepare("SELECT v.id,v.name,v.state FROM vaults v JOIN vault_readers r ON r.vault_id=v.id WHERE r.node_id=?1 ORDER BY v.name,v.id LIMIT 1000").map_err(db_error)?;
   let rows=q.query_map([node],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(db_error)?;
   rows.map(|r|{let(id,name,state)=r.map_err(db_error)?;Ok(VaultInfo{id:id.parse().map_err(|_|rejected("invalid vault identity"))?,name,state})}).collect()
  })
    }
    pub fn vault_status(&self, vault: Uuid) -> Result<VaultInfo> {
        self.with_node(|tx, node| self.vault_access(tx, node, vault, false))
    }
    pub fn vault_read(&self, vault: Uuid, id: Uuid, revision: &str) -> Result<VaultDocument> {
        self.with_node(|tx,node|{
   self.vault_access(tx,node,vault,true)?;
   let row:Option<(String,String,String,String)>=tx.query_row("SELECT path,revision,title,content FROM vault_documents WHERE id=?1 AND vault_id=?2 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=vault_documents.id AND a.vault_id=vault_documents.vault_id)",params![id.to_string(),vault.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
   let(path,current,title,content)=row.ok_or_else(||rejected("document not found"))?;
   if current!=revision {return Err(rejected("document revision changed; search again"))}
   Ok(VaultDocument{id,vault_id:vault,path,revision:current,title,content})
  })
    }
    pub fn vault_search(&self, vault: Uuid, query: &str, limit: u32) -> Result<Vec<VaultHit>> {
        check_text(query, 2048)?;
        if !(1..=100).contains(&limit) {
            return Err(rejected("search limit must be 1..100"));
        }
        // Treat user text as literal terms, not executable FTS query syntax.
        let terms: Vec<_> = query
            .split_whitespace()
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        let query = terms.join(" AND ");
        self.with_node(|tx,node|{
   self.vault_access(tx,node,vault,true)?;
   let mut q=tx.prepare("SELECT d.id,d.path,d.revision,d.title,snippet(vault_fts,1,'','', ' … ',32),bm25(vault_fts),d.document_date FROM vault_fts JOIN vault_documents d ON d.rowid=vault_fts.rowid WHERE vault_fts MATCH ?1 AND d.vault_id=?2 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=d.id AND a.vault_id=d.vault_id) ORDER BY bm25(vault_fts),d.id LIMIT ?3").map_err(db_error)?;
   let rows=q.query_map(params![query,vault.to_string(),limit],|r|Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get::<_,Option<i64>>(6)?))).map_err(db_error)?;
   rows.map(|r|{let(id,path,revision,title,snippet,score,document_date)=r.map_err(db_error)?;Ok(VaultHit{id:id.parse().map_err(|_|rejected("invalid document identity"))?,path,revision,title,snippet,score,document_date})}).collect()
  })
    }

    /// Lists a vault's documents without requiring a search term first -- the browse view the
    /// GUI needs (Jack, 2026-09-26: "I don't want to have to search in order to see results...
    /// like a Finder window where I can see all the artifacts we've catalogued"). Ordered by
    /// title so it reads like a sorted file listing. `snippet` is a best-effort content preview
    /// (first ~96 chars, not an FTS match highlight) and `score` is unused (0.0) since there is
    /// no ranking to report for an unfiltered listing -- both fields exist only so this can
    /// reuse `VaultHit` and the same list-rendering code the GUI already has for search results.
    pub fn vault_list_documents(&self, vault: Uuid, limit: u32) -> Result<Vec<VaultHit>> {
        if !(1..=1000).contains(&limit) {
            return Err(rejected("list limit must be 1..1000"));
        }
        self.with_node(|tx, node| {
            self.vault_access(tx, node, vault, true)?;
            let mut q = tx
                .prepare(
                    "SELECT id,path,revision,title,content,document_date FROM vault_documents WHERE vault_id=?1 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=vault_documents.id AND a.vault_id=vault_documents.vault_id) ORDER BY title,path LIMIT ?2",
                )
                .map_err(db_error)?;
            let rows = q
                .query_map(params![vault.to_string(), limit], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                    ))
                })
                .map_err(db_error)?;
            rows.map(|r| {
                let (id, path, revision, title, content, document_date) = r.map_err(db_error)?;
                let snippet: String = content.chars().take(96).collect();
                Ok(VaultHit {
                    id: id
                        .parse()
                        .map_err(|_| rejected("invalid document identity"))?,
                    path,
                    revision,
                    title,
                    snippet,
                    score: 0.0,
                    document_date,
                })
            })
            .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_date_is_parsed_from_frontmatter_or_sparks_raw_export_and_absent_otherwise() {
        // Frontmatter convention (loki-library-notes sweep, general Library intake).
        assert_eq!(
            extract_document_date("---\ntitle: X\ndate: 2026-09-27\ntags: a\n---\nbody"),
            Some(
                chrono::NaiveDate::from_ymd_opt(2026, 9, 27)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
            )
        );
        // Spark's raw export: no frontmatter, a bare "Date: " line near the top.
        assert_eq!(
            extract_document_date("# Q3 planning\n\nSource: Spark meeting 42\n\nMeeting: Q3 planning\nDate: 2026-09-27 14:30\nAttendees: ..."),
            Some(
                chrono::NaiveDate::from_ymd_opt(2026, 9, 27)
                    .unwrap()
                    .and_hms_opt(14, 30, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
            )
        );
        // Neither convention present: no date, not an error.
        assert_eq!(extract_document_date("# Just a note\n\nNo date anywhere in here."), None);
        // A "Date:" line past the first 20 lines doesn't count -- keeps the scan cheap and avoids
        // picking up an unrelated date mentioned deep in a long document's body.
        let mut far = "# Title\n".to_string();
        for _ in 0..25 {
            far.push_str("filler line\n");
        }
        far.push_str("Date: 2026-01-01\n");
        assert_eq!(extract_document_date(&far), None);
    }
    #[test]
    fn vault_put_stores_extracted_document_date_and_read_paths_return_it() {
        let s = LocalHubStore::in_memory().unwrap();
        let v = s.vault_create("Meetings").unwrap();
        s.vault_set_available(v, true).unwrap();
        let doc = Uuid::new_v4();
        s.vault_put(v, doc, "meeting.md", "Q3 sync", "Meeting: Q3 sync\nDate: 2026-09-27 14:30\n\nNotes here.")
            .unwrap();
        let expected = chrono::NaiveDate::from_ymd_opt(2026, 9, 27)
            .unwrap()
            .and_hms_opt(14, 30, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let listed = s.vault_list_documents_local(v, 10).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].document_date, Some(expected));
        let searched = s.vault_search_local(v, "Q3", 10).unwrap();
        assert_eq!(searched.len(), 1);
        assert_eq!(searched[0].document_date, Some(expected));
        // A document with no recognizable date reads back as None, not an error.
        let plain = Uuid::new_v4();
        s.vault_put(v, plain, "plain.md", "Plain", "No date convention here.")
            .unwrap();
        let plain_hit = s
            .vault_list_documents_local(v, 10)
            .unwrap()
            .into_iter()
            .find(|h| h.id == plain)
            .unwrap();
        assert_eq!(plain_hit.document_date, None);
    }
    #[test]
    fn computer_sharing_is_explicit_scoped_and_revocable() {
        let s = LocalHubStore::in_memory().unwrap();
        let a = s.enroll_owner("Same name").unwrap();
        let b = s.enroll_owner("Same name").unwrap();
        let reader = s.connect(&a.raw_key).unwrap();
        let v = s.vault_create("Shared").unwrap();
        let private = s.vault_create("Private").unwrap();
        let doc = Uuid::new_v4();
        let revision = s.vault_put(v, doc, "note.md", "Note", "hello").unwrap();
        s.vault_set_available(v, true).unwrap();
        assert!(reader.vault_list().unwrap().is_empty());
        assert_eq!(s.vault_computer_access(v).unwrap().len(), 2);
        s.vault_set_computer_access(v, a.node_id, true).unwrap();
        s.vault_set_computer_access(v, a.node_id, true).unwrap();
        assert!(reader.vault_read(v, doc, &revision).is_ok());
        assert_eq!(reader.vault_list().unwrap().len(), 1);
        assert!(!s
            .vault_computer_access(private)
            .unwrap()
            .iter()
            .any(|r| r.allowed));
        assert!(
            !s.vault_computer_access(v)
                .unwrap()
                .iter()
                .find(|r| r.node_id == b.node_id)
                .unwrap()
                .allowed
        );
        s.vault_set_computer_access(v, a.node_id, false).unwrap();
        assert!(reader.vault_read(v, doc, &revision).is_err());
        s.revoke(a.node_id).unwrap();
        assert!(s.vault_set_computer_access(v, a.node_id, true).is_err());
        assert!(s.vault_set_computer_access(v, a.node_id, false).is_ok());
        assert!(s
            .vault_set_computer_access(v, Uuid::new_v4(), true)
            .is_err());
        assert!(s.vault_computer_access(Uuid::new_v4()).is_err());
    }

    #[test]
    fn vault_grants_revisions_and_fts_remain_consistent() {
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("reader").unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let v = s.vault_create("Notes").unwrap();
        let other = s.vault_create("Secret").unwrap();
        assert_eq!(s.vault_list_all().unwrap().len(), 2);
        let id = Uuid::new_v4();
        let rev = s.vault_put(v, id, "note.md", "Plan", "alpha fox").unwrap();
        s.vault_put(other, Uuid::new_v4(), "secret.md", "Hidden", "alpha secret")
            .unwrap();
        assert!(h.vault_list().unwrap().is_empty());
        assert!(h.vault_read(v, id, &rev).is_err());
        s.vault_grant(v, c.node_id, true).unwrap();
        assert_eq!(h.vault_list().unwrap().len(), 1);
        assert!(matches!(
            h.vault_search(v, "alpha", 10),
            Err(HubError::Transport(_))
        ));
        s.vault_set_available(v, true).unwrap();
        assert_eq!(h.vault_search(v, "alpha", 10).unwrap().len(), 1);
        assert_eq!(h.vault_read(v, id, &rev).unwrap().content, "alpha fox");
        assert!(s.vault_put(other, id, "stolen.md", "x", "x").is_err());
        let next = s
            .vault_put(v, id, "renamed.md", "Plan", "beta fox")
            .unwrap();
        assert_ne!(next, rev);
        assert!(h.vault_read(v, id, &rev).is_err());
        assert!(h.vault_search(v, "alpha", 10).unwrap().is_empty());
        assert_eq!(h.vault_search(v, "beta", 10).unwrap()[0].id, id);
        assert_eq!(h.vault_read(v, id, &next).unwrap().path, "renamed.md");
        s.vault_remove_document(other, id).unwrap();
        assert!(h.vault_read(v, id, &next).is_ok());
        s.vault_remove_document(v, id).unwrap();
        assert!(h.vault_search(v, "beta", 10).unwrap().is_empty());
        s.vault_grant(v, c.node_id, false).unwrap();
        assert!(h.vault_status(v).is_err());
        s.vault_grant(v, c.node_id, true).unwrap();
        s.revoke(c.node_id).unwrap();
        assert!(matches!(h.vault_list(), Err(HubError::BadKey)));
    }
    #[test]
    fn vault_reopen_republishes_manual_vaults_but_not_folder_backed_ones() {
        let s = LocalHubStore::in_memory().unwrap();
        let manual = s.vault_create("Manual").unwrap();
        s.vault_set_available(manual, true).unwrap();
        let folder = s.vault_create("Folder").unwrap();
        s.vault_set_available(folder, true).unwrap();
        {
            let db = s.db.lock().unwrap();
            db.execute(
                "INSERT INTO vault_sources(vault_id,root,root_identity) VALUES(?1,'x','x')",
                [folder.to_string()],
            )
            .unwrap();
        }
        // Simulate the reopen every store-open performs (from_connection's own blanket reset).
        s.transaction(|tx| {
            tx.execute("UPDATE vaults SET state='unavailable'", [])
                .map_err(db_error)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            s.vault_list_all()
                .unwrap()
                .iter()
                .find(|v| v.id == manual)
                .unwrap()
                .state,
            "unavailable"
        );
        assert_eq!(
            s.vault_list_all()
                .unwrap()
                .iter()
                .find(|v| v.id == folder)
                .unwrap()
                .state,
            "unavailable"
        );
        s.vault_reopen_manual().unwrap();
        assert_eq!(
            s.vault_list_all()
                .unwrap()
                .iter()
                .find(|v| v.id == manual)
                .unwrap()
                .state,
            "ready"
        );
        // Folder-backed vault stays unavailable -- its own watcher/reconciliation republishes it,
        // never this blanket call.
        assert_eq!(
            s.vault_list_all()
                .unwrap()
                .iter()
                .find(|v| v.id == folder)
                .unwrap()
                .state,
            "unavailable"
        );
    }
    /// The bug the previous test's manual simulation was standing in for: a fresh
    /// `LocalHubStore::open` (not `in_memory`) against a real database file must republish
    /// every hand-curated vault as ready ON ITS OWN, with no separate call. A short-lived CLI
    /// process (`hive hub vault put`, say) does exactly this open/close cycle every single
    /// invocation while sharing the database file with a long-running `hive hub serve` -- so
    /// without this, every such CLI invocation would silently strand every hand-curated
    /// collection as unreadable over the paired-node RPC path until something else happened to
    /// call `vault_reopen_manual` by hand, which nothing in the real binary ever did.
    #[test]
    fn a_fresh_open_of_a_real_database_file_republishes_hand_curated_vaults_automatically() {
        let path = std::env::temp_dir().join(format!(
            "hive-vault-reopen-test-{}-{}.sqlite3",
            std::process::id(),
            Uuid::new_v4()
        ));
        let _ = std::fs::remove_file(&path);

        let (manual, folder) = {
            let s = LocalHubStore::open(&path).unwrap();
            let manual = s.vault_create("Manual").unwrap();
            s.vault_set_available(manual, true).unwrap();
            let folder = s.vault_create("Folder").unwrap();
            s.vault_set_available(folder, true).unwrap();
            {
                let db = s.db.lock().unwrap();
                db.execute(
                    "INSERT INTO vault_sources(vault_id,root,root_identity) VALUES(?1,'x','x')",
                    [folder.to_string()],
                )
                .unwrap();
            }
            (manual, folder)
        }; // Store (and its connection) dropped here -- simulates the CLI process exiting.

        // A second, independent open against the same file -- simulates the next CLI
        // invocation, or `hive hub serve` starting up after the CLI already touched the file.
        let s2 = LocalHubStore::open(&path).unwrap();
        let vaults = s2.vault_list_all().unwrap();
        assert_eq!(
            vaults.iter().find(|v| v.id == manual).unwrap().state,
            "ready",
            "a hand-curated vault must come back ready on its own, with no manual step"
        );
        assert_eq!(
            vaults.iter().find(|v| v.id == folder).unwrap().state,
            "unavailable",
            "a folder-backed vault still waits for its own watcher/reconciliation"
        );

        drop(s2);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn vault_migrates_existing_database_and_reopens() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("schema.sql")).unwrap();
        db.execute(
            "INSERT INTO projects(id,title,goal) VALUES('existing','Keep','Keep')",
            [],
        )
        .unwrap();
        let s = LocalHubStore::from_connection(db).unwrap();
        let db = s.db.lock().unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            // Room titles (v10) then bots_causation_schema.sql (v11, Track A slice 2)
            // migrate existing databases to schema version 21 (coding readiness).
            crate::local_hub::MIGRATIONS.len() as i64
        );
        assert_eq!(
            db.query_row("SELECT title FROM projects WHERE id='existing'", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "Keep"
        );
        drop(db);
        let conn = Arc::try_unwrap(s.db).unwrap().into_inner().unwrap();
        LocalHubStore::from_connection(conn).unwrap();
    }
    #[test]
    fn vault_rejects_unsafe_paths_and_bounds_queries() {
        for p in [
            "../x.md",
            "/x.md",
            "a/../x.md",
            "a\\x.md",
            "a//x.md",
            "C:x.md",
            "x.txt",
        ] {
            assert!(!path_ok(p), "{p}");
        }
        assert!(path_ok("folder/note.md"));
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("reader").unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let v = s.vault_create("Notes").unwrap();
        s.vault_grant(v, c.node_id, true).unwrap();
        s.vault_set_available(v, true).unwrap();
        assert!(h.vault_search(v, "a", 101).is_err());
        assert!(h.vault_search(v, " ", 10).is_err());
        assert!(h.vault_search(v, "\" OR *", 10).unwrap().is_empty());
    }

    #[test]
    fn vault_rename_updates_name_and_rejects_unknown_vault() {
        let s = LocalHubStore::in_memory().unwrap();
        let v = s.vault_create("Original").unwrap();
        s.vault_rename(v, "Renamed").unwrap();
        let info = s.vault_list_all().unwrap();
        let renamed = info.iter().find(|i| i.id == v).unwrap();
        assert_eq!(renamed.name, "Renamed");
        assert!(s.vault_rename(Uuid::new_v4(), "Nope").is_err());
        assert!(s.vault_rename(v, &"x".repeat(500)).is_err());
    }

    #[test]
    fn vault_delete_removes_catalog_state_and_rejects_unknown_vault() {
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("reader").unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let v = s.vault_create("Doomed").unwrap();
        let id = Uuid::new_v4();
        s.vault_put(v, id, "note.md", "Plan", "alpha fox").unwrap();
        s.vault_grant(v, c.node_id, true).unwrap();
        s.vault_set_available(v, true).unwrap();
        assert_eq!(s.vault_list_all().unwrap().len(), 1);
        assert_eq!(h.vault_list().unwrap().len(), 1);

        s.vault_delete(v).unwrap();

        assert!(s.vault_list_all().unwrap().is_empty());
        assert!(h.vault_status(v).is_err());
        assert!(s.vault_delete(v).is_err());
        assert!(s.vault_delete(Uuid::new_v4()).is_err());
    }
}
