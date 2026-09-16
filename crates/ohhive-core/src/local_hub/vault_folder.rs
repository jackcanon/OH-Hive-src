//! Read-only, bounded local Markdown ingestion. Cloud placeholders are not supported.
//! All filesystem access and parsing happens before the atomic database reconciliation.
use super::*;
use cap_std::fs::Dir;
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    time::Duration,
};

const MAX_FILE: u64 = 4 * 1024 * 1024;
const MAX_TOTAL: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MAX_NOTES: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultScanStatus {
    pub generation: i64,
    pub indexed_at: Option<i64>,
    pub last_error: Option<String>,
    /// Publication closure entry through transaction return, including commit; excludes lock wait.
    /// Runtime diagnostic only, not persisted. Includes negligible return/unlock overhead.
    #[serde(default)]
    pub publication_transaction_ms: Option<f64>,
}
#[derive(Clone, PartialEq, Eq)]
struct Note {
    path: String,
    title: String,
    content: String,
}
fn io_error(_: std::io::Error) -> HubError {
    rejected("vault source unavailable or unreadable")
}
fn root_identity(dir: &Dir) -> Result<String> {
    let m = dir.dir_metadata().map_err(io_error)?;
    #[cfg(unix)]
    {
        use cap_std::fs::MetadataExt;
        Ok(format!("{}:{}", m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        // Fail closed on filesystems that cannot supply a persistent creation time.
        Ok(format!("{:?}", m.created().map_err(io_error)?))
    }
}
fn open_root(root: &Path) -> Result<Dir> {
    if std::fs::symlink_metadata(root)
        .map_err(io_error)?
        .file_type()
        .is_symlink()
    {
        return Err(rejected("vault root cannot be a symlink"));
    }
    Dir::open_ambient_dir(root, cap_std::ambient_authority()).map_err(io_error)
}
fn snapshot(root: &Path, identity: &str) -> Result<Vec<Note>> {
    let dir = open_root(root)?;
    if root_identity(&dir)? != identity {
        return Err(rejected("vault source was replaced or unmounted"));
    }
    let mut out = Vec::new();
    let mut entries = 0;
    let mut total = 0;
    walk(&dir, Path::new(""), 0, &mut entries, &mut total, &mut out)?;
    // Check the path still resolves to the selected directory after the walk.
    if root_identity(&open_root(root)?)? != identity {
        return Err(rejected("vault source changed during scan"));
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}
fn walk(
    dir: &Dir,
    prefix: &Path,
    depth: usize,
    count: &mut usize,
    total: &mut usize,
    out: &mut Vec<Note>,
) -> Result<()> {
    if depth > 32 {
        return Err(rejected("vault exceeds directory depth limit"));
    }
    for entry in dir.entries().map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        *count += 1;
        if *count > MAX_ENTRIES {
            return Err(rejected("vault exceeds entry limit"));
        }
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| rejected("vault filename is not UTF-8"))?;
        // Hidden directories include .git, .obsidian and editor metadata.
        if name.starts_with('.') {
            continue;
        }
        let meta = dir.symlink_metadata(name).map_err(io_error)?;
        if meta.file_type().is_symlink() {
            return Err(rejected("vault contains a symlink"));
        }
        let path = prefix.join(name);
        if meta.is_dir() {
            let child = dir.open_dir(name).map_err(io_error)?;
            walk(&child, &path, depth + 1, count, total, out)?;
        } else if name.ends_with(".md") {
            if !meta.is_file() {
                return Err(rejected("vault note must be a regular file"));
            }
            let relative = path
                .iter()
                .map(|c| c.to_str().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("/");
            if !super::vault::path_ok(&relative) {
                return Err(rejected("invalid vault document path"));
            }
            let file = dir.open(name).map_err(io_error)?;
            let before = file.metadata().map_err(io_error)?;
            if !before.is_file() || before.len() > MAX_FILE {
                return Err(rejected("vault note exceeds size limit"));
            }
            let mut bytes = Vec::new();
            (&file)
                .take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(io_error)?;
            let after = file.metadata().map_err(io_error)?;
            if bytes.len() as u64 > MAX_FILE
                || before.len() != after.len()
                || before.modified().map_err(io_error)? != after.modified().map_err(io_error)?
            {
                return Err(rejected("vault note changed during scan or exceeds limit"));
            }
            *total += bytes.len();
            if *total > MAX_TOTAL || out.len() >= MAX_NOTES {
                return Err(rejected("vault exceeds corpus limit"));
            }
            let content =
                String::from_utf8(bytes).map_err(|_| rejected("vault note is not UTF-8"))?;
            let title = content
                .lines()
                .find_map(|line| line.strip_prefix("# "))
                .unwrap_or(name)
                .chars()
                .take(512)
                .collect();
            out.push(Note {
                path: relative,
                title,
                content,
            });
        }
    }
    Ok(())
}
impl LocalHubStore {
    /// Explicit owner selection only. Never changes or creates files in the source folder.
    pub fn vault_attach_folder(&self, vault: Uuid, root: impl AsRef<Path>) -> Result<()> {
        let root = root.as_ref();
        let dir = open_root(root)?;
        let identity = root_identity(&dir)?;
        let root = std::fs::canonicalize(root).map_err(io_error)?;
        let root = root
            .to_str()
            .ok_or_else(|| rejected("vault root is not UTF-8"))?;
        self.transaction(|tx| {
            let count: i64 = tx
                .query_row(
                    "SELECT count(*) FROM vault_documents WHERE vault_id=?1",
                    [vault.to_string()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if count != 0 {
                return Err(rejected("attach a folder to a new empty vault"));
            }
            tx.execute(
                "INSERT INTO vault_sources(vault_id,root,root_identity) VALUES(?1,?2,?3)",
                params![vault.to_string(), root, identity],
            )
            .map_err(db_error)?;
            tx.execute(
                "UPDATE vaults SET state='unavailable' WHERE id=?1",
                [vault.to_string()],
            )
            .map_err(db_error)?;
            Ok(())
        })
    }
    pub fn vault_scan_status(&self, vault: Uuid) -> Result<VaultScanStatus> {
        self.transaction(|tx| {
            tx.query_row(
                "SELECT generation,indexed_at,last_error FROM vault_sources WHERE vault_id=?1",
                [vault.to_string()],
                |r| {
                    Ok(VaultScanStatus {
                        generation: r.get(0)?,
                        indexed_at: r.get(1)?,
                        last_error: r.get(2)?,
                        publication_transaction_ms: None,
                    })
                },
            )
            .map_err(db_error)
        })
    }
    /// Full recovery scan; repeat scans must observe identical contents before publication.
    /// Concurrent scans use a generation check so an older snapshot cannot overwrite a newer one.
    pub fn vault_scan_folder(&self, vault: Uuid) -> Result<VaultScanStatus> {
        self.scan_folder(vault, None)
    }
    fn scan_folder(
        &self,
        vault: Uuid,
        active: Option<Arc<Mutex<bool>>>,
    ) -> Result<VaultScanStatus> {
        let (root, identity, generation): (String, String, i64) = self.transaction(|tx| {
            tx.query_row(
                "SELECT root,root_identity,generation FROM vault_sources WHERE vault_id=?1",
                [vault.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db_error)
        })?;
        let result = (|| {
            let notes = snapshot(Path::new(&root), &identity)?;
            if notes != snapshot(Path::new(&root), &identity)? {
                return Err(rejected("vault changed during scan; retry pending"));
            }
            // Serialize publication with watcher cancellation, including task abort.
            let guard = active
                .as_ref()
                .map(|a| {
                    a.lock()
                        .map_err(|_| rejected("vault watcher lock poisoned"))
                })
                .transpose()?;
            if guard.as_deref().is_some_and(|enabled| !enabled) {
                return Err(rejected("vault watcher stopped"));
            }
            self.reconcile_folder(vault, generation, &notes)
        })();
        if result.is_err() {
            self.transaction(|tx| {
                // Do not let an older failing scan invalidate a newer successful generation.
                tx.execute("UPDATE vaults SET state='unavailable' WHERE id=?1 AND EXISTS(SELECT 1 FROM vault_sources WHERE vault_id=?1 AND generation=?2)",params![vault.to_string(),generation]).map_err(db_error)?;
                tx.execute("UPDATE vault_sources SET generation=generation+1,last_error='Source scan failed; existing index preserved' WHERE vault_id=?1 AND generation=?2",params![vault.to_string(),generation]).map_err(db_error)?;
                Ok(())
            })?;
        }
        result
    }
    fn reconcile_folder(
        &self,
        vault: Uuid,
        generation: i64,
        notes: &[Note],
    ) -> Result<VaultScanStatus> {
        let old = self.transaction(|tx| {
            let mut q = tx
                .prepare("SELECT path,id,content,revision FROM vault_documents WHERE vault_id=?1")
                .map_err(db_error)?;
            let old: BTreeMap<String, (String, String, String)> = q
                .query_map([vault.to_string()], |r| {
                    Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
                })
                .map_err(db_error)?
                .collect::<std::result::Result<_, _>>()
                .map_err(db_error)?;
            Ok(old)
        })?;
        let incoming: std::collections::HashSet<_> =
            notes.iter().map(|n| n.path.as_str()).collect();
        let mut removed: HashMap<&str, Vec<&str>> = HashMap::new();
        for (path, (id, content, _)) in &old {
            if !incoming.contains(path.as_str()) {
                removed.entry(content).or_default().push(id);
            }
        }
        let mut added: HashMap<&str, usize> = HashMap::new();
        for n in notes {
            if !old.contains_key(&n.path) {
                *added.entry(&n.content).or_default() += 1;
            }
        }
        let mut rows = Vec::new();
        for n in notes {
            let id = if let Some((id, _, _)) = old.get(&n.path) {
                id.clone()
            } else if added.get(n.content.as_str()) == Some(&1)
                && removed
                    .get(n.content.as_str())
                    .is_some_and(|v| v.len() == 1)
            {
                removed[n.content.as_str()][0].to_owned()
            } else {
                Uuid::new_v4().to_string()
            };
            let parsed: Uuid = id
                .parse()
                .map_err(|_| rejected("invalid document identity"))?;
            let revision = digest(&encode(&(parsed, &n.path, &n.title, &n.content))?);
            rows.push((id, revision, n));
        }
        // Preserve all rows whose revision matches; delete changed/removed identities before
        // inserting replacements so rename/path collisions cannot violate the UNIQUE constraint.
        let keep: std::collections::HashSet<_> = rows
            .iter()
            .filter(|(_, revision, n)| {
                old.get(&n.path)
                    .is_some_and(|(_, _, previous)| previous == revision)
            })
            .map(|(id, _, _)| id.clone())
            .collect();
        let removed_ids: Vec<_> = old
            .values()
            .filter(|(id, _, _)| !keep.contains(id))
            .map(|(id, _, _)| id.clone())
            .collect();
        rows.retain(|(id, _, _)| !keep.contains(id));
        let mut publication_start = None;
        let mut status = self.transaction(|tx| {
            publication_start = Some(std::time::Instant::now());
            let current:i64 = tx.query_row("SELECT generation FROM vault_sources WHERE vault_id=?1",[vault.to_string()],|r|r.get(0)).map_err(db_error)?;
            if current!=generation {return Err(rejected("superseded vault scan"));}
            for id in removed_ids {
                tx.execute("DELETE FROM vault_documents WHERE vault_id=?1 AND id=?2",params![vault.to_string(),id]).map_err(db_error)?;
            }
            for (id,revision,n) in rows {
                tx.execute("INSERT INTO vault_documents(id,vault_id,path,revision,title,content) VALUES(?1,?2,?3,?4,?5,?6)",params![id,vault.to_string(),n.path,revision,n.title,n.content]).map_err(db_error)?;
            }
            let indexed_at=now();
            tx.execute("UPDATE vault_sources SET generation=generation+1,indexed_at=?2,last_error=NULL WHERE vault_id=?1",params![vault.to_string(),indexed_at]).map_err(db_error)?;
            tx.execute("UPDATE vaults SET state='ready' WHERE id=?1",[vault.to_string()]).map_err(db_error)?;
            Ok(VaultScanStatus{generation:generation+1,indexed_at:Some(indexed_at),last_error:None,publication_transaction_ms:None})
        })?;
        status.publication_transaction_ms =
            publication_start.map(|t| t.elapsed().as_secs_f64() * 1000.);
        Ok(status)
    }
    /// Portable polling watcher: initial scan plus periodic full reconciliation recovers missed edits.
    /// Caller owns this future and must shut it down before shutting down the hub.
    pub async fn vault_watch_folder(
        &self,
        vault: Uuid,
        interval: Duration,
        mut stop: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        if interval < Duration::from_millis(100) {
            return Err(rejected("scan interval too short"));
        }
        let active = Arc::new(Mutex::new(true));
        let _lifetime = WatchLifetime {
            store: self.clone(),
            vault,
            active: active.clone(),
        };
        loop {
            if *stop.borrow() {
                break;
            }
            let store = self.clone();
            let running = active.clone();
            // Await scans even when shutdown arrives; no detached scan may publish after shutdown.
            let _scan_result =
                tokio::task::spawn_blocking(move || store.scan_folder(vault, Some(running)))
                    .await
                    .map_err(|_| rejected("vault scan task failed"))?;
            tokio::select! {
                _=tokio::time::sleep(interval)=>{},
                changed=stop.changed()=>{if changed.is_err() || *stop.borrow(){break;}}
            }
        }
        self.vault_set_available(vault, false)
    }
}

struct WatchLifetime {
    store: LocalHubStore,
    vault: Uuid,
    active: Arc<Mutex<bool>>,
}
impl Drop for WatchLifetime {
    fn drop(&mut self) {
        // The same lock surrounds publication, so an in-flight scan cannot restore ready later.
        if let Ok(mut active) = self.active.lock() {
            *active = false;
            let _ = self.store.vault_set_available(self.vault, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("hive-vault-{}", Uuid::new_v4()));
            std::fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn setup(root: &Path) -> (LocalHubStore, LocalHub, Uuid) {
        let s = LocalHubStore::in_memory().unwrap();
        let c = s.enroll_owner("reader").unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        let v = s.vault_create("fixture").unwrap();
        s.vault_grant(v, c.node_id, true).unwrap();
        s.vault_attach_folder(v, root).unwrap();
        (s, h, v)
    }
    #[test]
    fn scan_edits_renames_deletes_and_preserves_copy_identity() {
        let t = Temp::new();
        let p = t.0.join("one.md");
        std::fs::write(&p, "# First\nfox alpha").unwrap();
        let (s, h, v) = setup(&t.0);
        s.vault_scan_folder(v).unwrap();
        let first = h.vault_search(v, "fox", 10).unwrap().remove(0);
        std::fs::rename(&p, t.0.join("two.md")).unwrap();
        s.vault_scan_folder(v).unwrap();
        let renamed = h.vault_search(v, "fox", 10).unwrap().remove(0);
        assert_eq!(first.id, renamed.id);
        assert_ne!(first.revision, renamed.revision);
        assert!(h.vault_read(v, first.id, &first.revision).is_err());
        std::fs::write(t.0.join("two.md"), "# Changed\nfox beta").unwrap();
        s.vault_scan_folder(v).unwrap();
        assert!(h.vault_search(v, "alpha", 10).unwrap().is_empty());
        assert_eq!(h.vault_search(v, "beta", 10).unwrap()[0].id, first.id);
        std::fs::copy(t.0.join("two.md"), t.0.join("copy.md")).unwrap();
        s.vault_scan_folder(v).unwrap();
        let hits = h.vault_search(v, "beta", 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_ne!(hits[0].id, hits[1].id);
        std::fs::remove_file(t.0.join("two.md")).unwrap();
        s.vault_scan_folder(v).unwrap();
        assert_eq!(h.vault_search(v, "beta", 10).unwrap().len(), 1);
        assert!(s
            .vault_put(v, Uuid::new_v4(), "bypass.md", "x", "x")
            .is_err());
    }
    #[test]
    fn incomplete_or_missing_source_preserves_index_and_recovers() {
        let t = Temp::new();
        let root = t.0.join("notes");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.md"), "retained").unwrap();
        let (s, h, v) = setup(&root);
        s.vault_scan_folder(v).unwrap();
        std::fs::write(root.join("invalid.md"), [255]).unwrap();
        let generation = s.vault_scan_status(v).unwrap().generation;
        assert!(s.vault_scan_folder(v).is_err());
        assert_eq!(s.vault_scan_status(v).unwrap().generation, generation + 1);
        assert!(s.reconcile_folder(v, generation, &[]).is_err());
        assert!(h.vault_search(v, "retained", 10).is_err());
        let count: i64 = s
            .transaction(|tx| {
                Ok(tx
                    .query_row("SELECT count(*) FROM vault_documents", [], |r| r.get(0))
                    .unwrap())
            })
            .unwrap();
        assert_eq!(count, 1);
        std::fs::remove_file(root.join("invalid.md")).unwrap();
        std::fs::rename(&root, t.0.join("away")).unwrap();
        assert!(s.vault_scan_folder(v).is_err());
        std::fs::create_dir(&root).unwrap(); // Empty mountpoint must not look like all notes were deleted.
        assert!(s.vault_scan_folder(v).is_err());
        std::fs::remove_dir(&root).unwrap();
        std::fs::rename(t.0.join("away"), &root).unwrap();
        s.vault_scan_folder(v).unwrap();
        assert_eq!(h.vault_search(v, "retained", 10).unwrap().len(), 1);
        assert!(s.vault_scan_status(v).unwrap().last_error.is_none());
    }
    #[test]
    fn restart_retains_identity_but_requires_rescan() {
        let t = Temp::new();
        let root = t.0.join("notes");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.md"), "persistent").unwrap();
        let db = t.0.join("hub.sqlite");
        let s = LocalHubStore::open(&db).unwrap();
        let c = s.enroll_owner("reader").unwrap();
        let v = s.vault_create("notes").unwrap();
        s.vault_grant(v, c.node_id, true).unwrap();
        s.vault_attach_folder(v, &root).unwrap();
        s.vault_scan_folder(v).unwrap();
        let hit = s
            .connect(&c.raw_key)
            .unwrap()
            .vault_search(v, "persistent", 10)
            .unwrap()
            .remove(0);
        drop(s);
        let s = LocalHubStore::open(&db).unwrap();
        let h = s.connect(&c.raw_key).unwrap();
        assert!(h.vault_read(v, hit.id, &hit.revision).is_err());
        s.vault_scan_folder(v).unwrap();
        assert!(h.vault_read(v, hit.id, &hit.revision).is_ok());
    }
    #[test]
    fn schema_two_migrates_without_losing_documents() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("schema.sql")).unwrap();
        db.execute_batch(include_str!("vault_schema.sql")).unwrap();
        db.execute(
            "INSERT INTO vaults(id,name,state) VALUES('v','existing','ready')",
            [],
        )
        .unwrap();
        let s = LocalHubStore::from_connection(db).unwrap();
        s.transaction(|tx| {
            assert_eq!(
                tx.query_row("SELECT name FROM vaults WHERE id='v'", [], |r| r
                    .get::<_, String>(0))
                    .unwrap(),
                "existing"
            );
            assert_eq!(
                tx.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                // Room titles (v10) then bots_causation_schema.sql (v11, Track A slice 2)
                // migrate existing databases to schema version 11.
            11
            );
            Ok(())
        })
        .unwrap();
    }
    #[tokio::test]
    async fn aborting_watcher_marks_unavailable() {
        let t = Temp::new();
        let (s, h, v) = setup(&t.0);
        let store = s.clone();
        let (_stop, rx) = tokio::sync::watch::channel(false);
        let watcher = tokio::spawn(async move {
            store
                .vault_watch_folder(v, Duration::from_millis(100), rx)
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), async {
            while h.vault_status(v).unwrap().state != "ready" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        watcher.abort();
        let _ = watcher.await;
        assert_eq!(h.vault_status(v).unwrap().state, "unavailable");
    }
    #[test]
    fn stale_generation_cannot_replace_a_newer_index() {
        let t = Temp::new();
        std::fs::write(t.0.join("a.md"), "original").unwrap();
        let (s, h, v) = setup(&t.0);
        s.vault_scan_folder(v).unwrap();
        assert!(s.reconcile_folder(v, 0, &[]).is_err());
        assert_eq!(h.vault_search(v, "original", 10).unwrap().len(), 1);
    }
    #[test]
    fn hidden_metadata_ignored_and_large_notes_rejected() {
        let t = Temp::new();
        std::fs::create_dir(t.0.join(".git")).unwrap();
        std::fs::write(t.0.join(".git/secret.md"), "secret").unwrap();
        let (s, h, v) = setup(&t.0);
        s.vault_scan_folder(v).unwrap();
        assert!(h.vault_search(v, "secret", 10).unwrap().is_empty());
        std::fs::File::create(t.0.join("big.md"))
            .unwrap()
            .set_len(MAX_FILE + 1)
            .unwrap();
        assert!(s.vault_scan_folder(v).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_rejected() {
        let t = Temp::new();
        let outside = Temp::new();
        std::fs::write(outside.0.join("private.md"), "private").unwrap();
        let (s, h, v) = setup(&t.0);
        std::os::unix::fs::symlink(&outside.0, t.0.join("escape")).unwrap();
        assert!(s.vault_scan_folder(v).is_err());
        assert!(h.vault_search(v, "private", 10).is_err());
    }
    #[tokio::test]
    async fn polling_recovers_rapid_saves_and_shuts_down_unavailable() {
        let t = Temp::new();
        let (s, h, v) = setup(&t.0);
        let store = s.clone();
        let (stop, rx) = tokio::sync::watch::channel(false);
        let watcher = tokio::spawn(async move {
            store
                .vault_watch_folder(v, Duration::from_millis(100), rx)
                .await
        });
        for i in 0..20 {
            std::fs::write(t.0.join("save.tmp"), format!("revision{i}")).unwrap();
            std::fs::rename(t.0.join("save.tmp"), t.0.join("note.md")).unwrap();
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if h.vault_search(v, "revision19", 10)
                    .is_ok_and(|hits| hits.len() == 1)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        stop.send(true).unwrap();
        watcher.await.unwrap().unwrap();
        assert_eq!(h.vault_status(v).unwrap().state, "unavailable");
    }
    #[test]
    fn corpus_bytes_accept_exact_limit_and_reject_one_more() {
        let t = Temp::new();
        let chunk = vec![b'x'; MAX_FILE as usize];
        for i in 0..MAX_TOTAL / MAX_FILE as usize {
            std::fs::write(t.0.join(format!("{i}.md")), &chunk).unwrap();
        }
        let dir = open_root(&t.0).unwrap();
        let identity = root_identity(&dir).unwrap();
        assert_eq!(
            snapshot(&t.0, &identity)
                .unwrap()
                .iter()
                .map(|n| n.content.len())
                .sum::<usize>(),
            MAX_TOTAL
        );
        std::fs::write(t.0.join("overflow.md"), "x").unwrap();
        assert!(snapshot(&t.0, &identity).is_err());
    }
    #[test]
    fn note_count_accepts_exact_limit_and_rejects_one_more() {
        let t = Temp::new();
        for i in 0..MAX_NOTES {
            std::fs::write(t.0.join(format!("{i}.md")), "").unwrap();
        }
        let dir = open_root(&t.0).unwrap();
        let identity = root_identity(&dir).unwrap();
        assert_eq!(snapshot(&t.0, &identity).unwrap().len(), MAX_NOTES);
        std::fs::write(t.0.join("overflow.md"), "").unwrap();
        assert!(snapshot(&t.0, &identity).is_err());
    }
    #[test]
    fn entry_count_accepts_exact_limit_and_rejects_one_more() {
        let t = Temp::new();
        for i in 0..MAX_ENTRIES {
            std::fs::write(t.0.join(format!("{i}.txt")), "").unwrap();
        }
        let dir = open_root(&t.0).unwrap();
        let identity = root_identity(&dir).unwrap();
        assert!(snapshot(&t.0, &identity).unwrap().is_empty());
        std::fs::write(t.0.join("overflow.txt"), "").unwrap();
        assert!(snapshot(&t.0, &identity).is_err());
    }
    #[test]
    fn directory_depth_accepts_32_and_rejects_33() {
        let t = Temp::new();
        let mut nested = t.0.clone();
        for _ in 0..32 {
            nested = nested.join("d");
            std::fs::create_dir(&nested).unwrap();
        }
        std::fs::write(nested.join("a.md"), "deep").unwrap();
        let dir = open_root(&t.0).unwrap();
        let identity = root_identity(&dir).unwrap();
        assert_eq!(snapshot(&t.0, &identity).unwrap().len(), 1);
        std::fs::create_dir(nested.join("d")).unwrap();
        assert!(snapshot(&t.0, &identity).is_err());
    }
    #[test]
    fn unchanged_scan_does_not_touch_documents_and_recovers_availability() {
        let t = Temp::new();
        std::fs::write(t.0.join("a.md"), "unchanged").unwrap();
        let (s, h, v) = setup(&t.0);
        s.vault_scan_folder(v).unwrap();
        s.transaction(|tx|{tx.execute_batch("CREATE TABLE mutation_audit(n INTEGER); CREATE TRIGGER audit_delete AFTER DELETE ON vault_documents BEGIN INSERT INTO mutation_audit VALUES(1); END; CREATE TRIGGER audit_insert AFTER INSERT ON vault_documents BEGIN INSERT INTO mutation_audit VALUES(1); END; CREATE TRIGGER audit_update AFTER UPDATE ON vault_documents BEGIN INSERT INTO mutation_audit VALUES(1); END;").unwrap();Ok(())}).unwrap();
        let previous = s.vault_scan_status(v).unwrap().generation;
        s.vault_set_available(v, false).unwrap();
        s.vault_scan_folder(v).unwrap();
        assert_eq!(h.vault_search(v, "unchanged", 10).unwrap().len(), 1);
        assert_eq!(s.vault_scan_status(v).unwrap().generation, previous + 1);
        s.transaction(|tx| {
            assert_eq!(
                tx.query_row("SELECT count(*) FROM mutation_audit", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            Ok(())
        })
        .unwrap();
        assert!(s.reconcile_folder(v, previous, &[]).is_err());
    }
    #[test]
    fn incremental_scan_preserves_unedited_rows_and_handles_multiple_changes() {
        let t = Temp::new();
        for (path, text) in [
            ("keep.md", "keeper"),
            ("edit.md", "before"),
            ("remove.md", "remove"),
            ("move.md", "mover"),
        ] {
            std::fs::write(t.0.join(path), text).unwrap();
        }
        let (s, h, v) = setup(&t.0);
        s.vault_scan_folder(v).unwrap();
        let kept = h.vault_search(v, "keeper", 10).unwrap().remove(0);
        let moved = h.vault_search(v, "mover", 10).unwrap().remove(0);
        s.transaction(|tx| {tx.execute_batch("CREATE TABLE touched(id TEXT); CREATE TRIGGER deleted_doc AFTER DELETE ON vault_documents BEGIN INSERT INTO touched VALUES(old.id); END; CREATE TRIGGER inserted_doc AFTER INSERT ON vault_documents BEGIN INSERT INTO touched VALUES(new.id); END;").unwrap();Ok(())}).unwrap();
        std::fs::write(t.0.join("edit.md"), "after").unwrap();
        std::fs::remove_file(t.0.join("remove.md")).unwrap();
        std::fs::rename(t.0.join("move.md"), t.0.join("renamed.md")).unwrap();
        std::fs::write(t.0.join("new.md"), "newnote").unwrap();
        s.vault_scan_folder(v).unwrap();
        assert!(h.vault_read(v, kept.id, &kept.revision).is_ok());
        assert_eq!(h.vault_search(v, "mover", 10).unwrap()[0].id, moved.id);
        for word in ["before", "remove"] {
            assert!(h.vault_search(v, word, 10).unwrap().is_empty());
        }
        for word in ["after", "newnote"] {
            assert_eq!(h.vault_search(v, word, 10).unwrap().len(), 1);
        }
        s.transaction(|tx| {
            assert_eq!(
                tx.query_row(
                    "SELECT count(*) FROM touched WHERE id=?1",
                    [kept.id.to_string()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            assert_eq!(
                tx.query_row("SELECT count(*) FROM touched", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                6
            );
            Ok(())
        })
        .unwrap();
    }
}
