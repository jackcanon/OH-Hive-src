//! Private fleet vault storage. Administration is host-local; readers need an explicit grant.
//! Document identity survives path/content changes. Reads require the current indexed revision.
use super::*;
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
        self.transaction(|tx|{
   if tx.query_row("SELECT EXISTS(SELECT 1 FROM vault_sources WHERE vault_id=?1)",[vault.to_string()],|r|r.get::<_,bool>(0)).map_err(db_error)? {return Err(rejected("folder vault writes require reconciliation"));}
   let owner:Option<String>=tx.query_row("SELECT vault_id FROM vault_documents WHERE id=?1",[id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
   if owner.as_deref().is_some_and(|v|v!=vault.to_string()){return Err(rejected("document belongs to another vault"))}
   tx.execute("INSERT INTO vault_documents(id,vault_id,path,revision,title,content) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET path=excluded.path,revision=excluded.revision,title=excluded.title,content=excluded.content",params![id.to_string(),vault.to_string(),path,revision,title,content]).map_err(db_error)?;
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
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
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
    fn vault_access(
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
   let mut q=tx.prepare("SELECT d.id,d.path,d.revision,d.title,snippet(vault_fts,1,'','', ' … ',32),bm25(vault_fts) FROM vault_fts JOIN vault_documents d ON d.rowid=vault_fts.rowid WHERE vault_fts MATCH ?1 AND d.vault_id=?2 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=d.id AND a.vault_id=d.vault_id) ORDER BY bm25(vault_fts),d.id LIMIT ?3").map_err(db_error)?;
   let rows=q.query_map(params![query,vault.to_string(),limit],|r|Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(db_error)?;
   rows.map(|r|{let(id,path,revision,title,snippet,score)=r.map_err(db_error)?;Ok(VaultHit{id:id.parse().map_err(|_|rejected("invalid document identity"))?,path,revision,title,snippet,score})}).collect()
  })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(s.vault_list_all().unwrap().iter().find(|v| v.id == manual).unwrap().state, "unavailable");
        assert_eq!(s.vault_list_all().unwrap().iter().find(|v| v.id == folder).unwrap().state, "unavailable");
        s.vault_reopen_manual().unwrap();
        assert_eq!(s.vault_list_all().unwrap().iter().find(|v| v.id == manual).unwrap().state, "ready");
        // Folder-backed vault stays unavailable -- its own watcher/reconciliation republishes it,
        // never this blanket call.
        assert_eq!(s.vault_list_all().unwrap().iter().find(|v| v.id == folder).unwrap().state, "unavailable");
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
            // bots_schema.sql (C1, 2026-09-15) bumped current schema to 7 -- see
            // local_hub/mod.rs's from_connection. Sif caught this stale assertion in the
            // first combined bots+local-hub test run (CONTINUITY.md, 2026-09-15).
            7
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
}
