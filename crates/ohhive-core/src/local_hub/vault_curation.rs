//! Host-only, deterministic curation. No model, network, filesystem mutation or implicit archive.
//! Age means time since this index observed a revision, not document truth or source mtime.
//! Archive is an identity-bound overlay; source refresh cannot silently undo it. Remote readers
//! inherit the overlay through vault_search/read. Mutation/provenance APIs are not on transport.
use super::vault::VaultDocument;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const MAX_DOCUMENTS: i64 = 10_000;
const MAX_BYTES: i64 = 64 * 1024 * 1024;
const MAX_PAIRS: usize = 100_000;
const MAX_FINDINGS: usize = 1000;
const DAY_MS: i64 = 86_400_000;
fn curation_now() -> i64 {
    Utc::now().timestamp_millis()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationNote {
    pub id: Uuid,
    pub revision: String,
    pub path: String,
    pub changed_ms: i64,
    /// Managed-intake lineage, when available; never inferred from similar text.
    pub intake_source: Option<String>,
    pub intake_generation: Option<i64>,
    pub folder_backed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCandidate {
    pub left: Uuid,
    pub right: Uuid,
    /// `normalized_exact` ignores case/whitespace; `near` uses unique word overlap.
    pub kind: String,
    pub similarity: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationReport {
    pub notes: Vec<CurationNote>,
    pub stale: Vec<Uuid>,
    pub duplicates: Vec<DuplicateCandidate>,
    pub near_pairs_examined: usize,
    /// False when work/output caps prevent reporting all candidates. Never silently complete.
    pub duplicate_scan_complete: bool,
    pub observed_at_ms: i64,
    pub vault_state: String,
    pub stale_after_days: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurationEvent {
    pub sequence: i64,
    pub document_id: Uuid,
    pub at_ms: i64,
    pub action: String,
    pub revision: String,
    pub actor: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveSummary {
    pub document_id: Uuid,
    pub archived_revision: String,
    pub current_revision: Option<String>,
    pub archived_ms: i64,
    pub snapshot_retained: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceEvent {
    pub sequence: i64,
    pub at_ms: i64,
    pub kind: String,
    pub revision: String,
    pub path: String,
}

fn document(tx: &Transaction<'_>, vault: Uuid, id: Uuid) -> Result<Option<VaultDocument>> {
    tx.query_row(
        "SELECT path,revision,title,content FROM vault_documents WHERE vault_id=?1 AND id=?2",
        params![vault.to_string(), id.to_string()],
        |r| {
            Ok(VaultDocument {
                id,
                vault_id: vault,
                path: r.get(0)?,
                revision: r.get(1)?,
                title: r.get(2)?,
                content: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)
}
fn vault_exists(tx: &Transaction<'_>, vault: Uuid) -> Result<()> {
    if !tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM vaults WHERE id=?1)",
            [vault.to_string()],
            |r| r.get::<_, bool>(0),
        )
        .map_err(db_error)?
    {
        return Err(rejected("vault not found"));
    }
    Ok(())
}
fn record(
    tx: &Transaction<'_>,
    vault: Uuid,
    id: Uuid,
    action: &str,
    revision: &str,
    actor: &str,
    reason: &str,
) -> Result<()> {
    tx.execute("INSERT INTO vault_curation_events(vault_id,document_id,at_ms,action,revision,actor,reason) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![vault.to_string(),id.to_string(),curation_now(),action,revision,actor,reason]).map_err(db_error)?;
    Ok(())
}
fn attribution(actor: &str, reason: &str) -> Result<()> {
    check_text(actor, 200)?;
    check_text(reason, 2048)
}

impl LocalHubStore {
    /// Default recommendation is 90 days. Caller selects 1..3650. Reports never mutate notes.
    pub fn vault_curation_scan(
        &self,
        vault: Uuid,
        stale_after_days: u32,
    ) -> Result<CurationReport> {
        self.curation_scan_at(vault, stale_after_days, curation_now())
    }
    fn curation_scan_at(&self, vault: Uuid, days: u32, at: i64) -> Result<CurationReport> {
        if !(1..=3650).contains(&days) {
            return Err(rejected("staleness must be 1..3650 days"));
        }
        // Snapshot under the store lock; CPU comparison happens after the transaction releases it.
        let (vault_state, rows) = self.transaction(|tx| {
            vault_exists(tx,vault)?;
            let (count,bytes):(i64,i64)=tx.query_row("SELECT count(*),coalesce(sum(length(CAST(content AS BLOB))),0) FROM vault_documents d WHERE vault_id=?1 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=d.id)",[vault.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).map_err(db_error)?;
            if count>MAX_DOCUMENTS || bytes>MAX_BYTES {return Err(rejected("curation corpus exceeds 10000 notes or 64 MiB"));}
            let mut q=tx.prepare("SELECT d.id,d.revision,d.path,o.changed_ms,i.source_id,i.generation,EXISTS(SELECT 1 FROM vault_sources s WHERE s.vault_id=d.vault_id),d.content FROM vault_documents d JOIN vault_observations o ON o.document_id=d.id LEFT JOIN vault_intake i ON i.document_id=d.id AND i.vault_id=d.vault_id WHERE d.vault_id=?1 AND NOT EXISTS(SELECT 1 FROM vault_archives a WHERE a.document_id=d.id) ORDER BY d.id").map_err(db_error)?;
            let mapped=q.query_map([vault.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<i64>>(5)?,r.get::<_,bool>(6)?,r.get::<_,String>(7)?))).map_err(db_error)?;
            let rows=mapped.map(|row|{
                let(id,revision,path,changed_ms,intake_source,intake_generation,folder_backed,content)=row.map_err(db_error)?;
                Ok((CurationNote{id:id.parse().map_err(|_|rejected("invalid document id"))?,revision,path,changed_ms,intake_source,intake_generation,folder_backed},content))
            }).collect::<Result<Vec<_>>>()?;
            let state=tx.query_row("SELECT state FROM vaults WHERE id=?1",[vault.to_string()],|r|r.get::<_,String>(0)).map_err(db_error)?;
            Ok((state,rows))
        })?;
        let mut result = CurationReport {
            notes: Vec::new(),
            stale: Vec::new(),
            duplicates: Vec::new(),
            near_pairs_examined: 0,
            duplicate_scan_complete: true,
            observed_at_ms: at,
            vault_state,
            stale_after_days: days,
        };
        let mut exact = BTreeMap::<String, Uuid>::new();
        let mut signatures = Vec::<(Uuid, String, BTreeSet<String>)>::new();
        for (note, content) in rows {
            if at.saturating_sub(note.changed_ms) >= i64::from(days) * DAY_MS {
                result.stale.push(note.id);
            }
            let normalized = content
                .split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
                .join(" ");
            let fingerprint = digest(&normalized);
            if !normalized.is_empty() {
                if let Some(first) = exact.get(&fingerprint) {
                    if result.duplicates.len() < MAX_FINDINGS {
                        result.duplicates.push(DuplicateCandidate {
                            left: *first,
                            right: note.id,
                            kind: "normalized_exact".into(),
                            similarity: 1.0,
                        });
                    } else {
                        result.duplicate_scan_complete = false;
                    }
                } else {
                    exact.insert(fingerprint.clone(), note.id);
                }
            }
            // Bound per-document near-match memory. Longer notes still receive exact matching.
            let words: BTreeSet<_> = normalized
                .split_whitespace()
                .take(2049)
                .map(str::to_owned)
                .collect();
            if normalized.split_whitespace().count() <= 2048 && words.len() >= 20 {
                signatures.push((note.id, fingerprint, words));
            } else if normalized.split_whitespace().count() > 2048 {
                result.duplicate_scan_complete = false;
            }
            result.notes.push(note);
        }
        'pairs: for i in 0..signatures.len() {
            for j in i + 1..signatures.len() {
                if result.near_pairs_examined >= MAX_PAIRS
                    || result.duplicates.len() >= MAX_FINDINGS
                {
                    result.duplicate_scan_complete = false;
                    break 'pairs;
                }
                result.near_pairs_examined += 1;
                let (a, ha, wa) = &signatures[i];
                let (b, hb, wb) = &signatures[j];
                if ha == hb {
                    continue;
                }
                let intersection = wa.intersection(wb).count();
                let union = wa.len() + wb.len() - intersection;
                let score = intersection as f64 / union as f64;
                if score >= 0.85 {
                    result.duplicates.push(DuplicateCandidate {
                        left: *a,
                        right: *b,
                        kind: "near".into(),
                        similarity: score,
                    });
                }
            }
        }
        Ok(result)
    }
    /// Atomically retain a snapshot and hide this identity. Actor is host-supplied attribution,
    /// not authentication. Stale revision, repeated action or wrong vault fails without mutation.
    pub fn vault_archive(
        &self,
        vault: Uuid,
        id: Uuid,
        revision: &str,
        actor: &str,
        reason: &str,
    ) -> Result<()> {
        attribution(actor, reason)?;
        self.transaction(|tx|{
            let doc=document(tx,vault,id)?.ok_or_else(||rejected("document not found"))?;
            if doc.revision!=revision {return Err(rejected("document revision changed; review again"));}
            let snapshot=encode(&doc)?;
            super::vault_maintenance::reserve_archive(tx,vault,snapshot.len() as i64,curation_now())?;
            let bytes:i64=tx.query_row("SELECT coalesce(sum(length(CAST(snapshot AS BLOB))),0) FROM vault_archives WHERE vault_id=?1",[vault.to_string()],|r|r.get(0)).map_err(db_error)?;
            if bytes.saturating_add(snapshot.len() as i64)>MAX_BYTES {return Err(rejected("archive exceeds 64 MiB per vault"));}
            tx.execute("INSERT INTO vault_archives VALUES(?1,?2,?3,?4,?5)",params![id.to_string(),vault.to_string(),revision,snapshot,curation_now()]).map_err(db_error)?;
            record(tx,vault,id,"archive",revision,actor,reason)
        })
    }
    /// Restore visibility of the explicitly reviewed *current* revision. If a manual note was
    /// removed, restores its retained snapshot. Missing folder notes must be restored at source.
    /// A changed source is never overwritten with an older archive copy.
    pub fn vault_restore(
        &self,
        vault: Uuid,
        id: Uuid,
        expected_revision: &str,
        actor: &str,
        reason: &str,
    ) -> Result<()> {
        attribution(actor, reason)?;
        self.transaction(|tx|{
            let snapshot:Option<Option<String>>=tx.query_row("SELECT snapshot FROM vault_archives WHERE document_id=?1 AND vault_id=?2",params![id.to_string(),vault.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            let saved:Option<VaultDocument>=snapshot.ok_or_else(||rejected("archive not found"))?.map(|s|decode(&s)).transpose()?;
            let current=document(tx,vault,id)?;
            let revision=current.as_ref().map(|d|d.revision.as_str()).or_else(||saved.as_ref().map(|d|d.revision.as_str())).ok_or_else(||rejected("retained snapshot expired and source missing"))?;
            if revision!=expected_revision {return Err(rejected("restore revision changed; review again"));}
            if current.is_none() {
                let saved=saved.as_ref().ok_or_else(||rejected("snapshot expired"))?;
                let folder:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM vault_sources WHERE vault_id=?1)",[vault.to_string()],|r|r.get(0)).map_err(db_error)?;
                if folder {return Err(rejected("restore missing folder note at source before unarchiving"));}
                // Managed intake exclusion is authoritative; restoring must not bypass its tombstone.
                let excluded:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM vault_intake WHERE vault_id=?1 AND document_id=?2 AND revision IS NULL)",params![vault.to_string(),id.to_string()],|r|r.get(0)).map_err(db_error)?;
                if excluded {return Err(rejected("excluded intake source requires a new intake generation"));}
                tx.execute("INSERT INTO vault_documents(id,vault_id,path,revision,title,content) VALUES(?1,?2,?3,?4,?5,?6)",params![id.to_string(),vault.to_string(),saved.path,saved.revision,saved.title,saved.content]).map_err(db_error)?;
            }
            tx.execute("DELETE FROM vault_archives WHERE document_id=?1 AND vault_id=?2",params![id.to_string(),vault.to_string()]).map_err(db_error)?;
            record(tx,vault,id,"restore",expected_revision,actor,reason)
        })
    }
    /// Compact owner inventory; cursor is the last document_id from the previous page.
    pub fn vault_archives(
        &self,
        vault: Uuid,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<ArchiveSummary>> {
        if !(1..=100).contains(&limit) {
            return Err(rejected("invalid archive page"));
        }
        self.transaction(|tx| {
            vault_exists(tx,vault)?;
            let mut q=tx.prepare("SELECT a.document_id,a.revision,d.revision,a.archived_ms,a.snapshot IS NOT NULL FROM vault_archives a LEFT JOIN vault_documents d ON d.id=a.document_id AND d.vault_id=a.vault_id WHERE a.vault_id=?1 AND a.document_id>?2 ORDER BY a.document_id LIMIT ?3").map_err(db_error)?;
            let rows=q.query_map(params![vault.to_string(),after.map(|id|id.to_string()).unwrap_or_default(),limit],|r|Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(db_error)?;
            rows.map(|row|{let(id,archived_revision,current_revision,archived_ms,snapshot_retained)=row.map_err(db_error)?;Ok(ArchiveSummary{document_id:id.parse().map_err(|_|rejected("invalid id"))?,archived_revision,current_revision,archived_ms,snapshot_retained})}).collect()
        })
    }
    /// Host can inspect both archived snapshot and current revision through explicit APIs.
    pub fn vault_archive_snapshot(&self, vault: Uuid, id: Uuid) -> Result<VaultDocument> {
        self.transaction(|tx| {
            let value: Option<Option<String>> = tx
                .query_row(
                    "SELECT snapshot FROM vault_archives WHERE vault_id=?1 AND document_id=?2",
                    params![vault.to_string(), id.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            decode(
                &value
                    .flatten()
                    .ok_or_else(|| rejected("archive snapshot missing or expired"))?,
            )
        })
    }
    pub fn vault_curation_current(&self, vault: Uuid, id: Uuid) -> Result<Option<VaultDocument>> {
        self.transaction(|tx| document(tx, vault, id))
    }
    pub fn vault_curation_history(
        &self,
        vault: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<CurationEvent>> {
        if after < 0 || !(1..=100).contains(&limit) {
            return Err(rejected("invalid history page"));
        }
        self.transaction(|tx|{
            vault_exists(tx,vault)?;
            let mut q=tx.prepare("SELECT sequence,document_id,at_ms,action,revision,actor,reason FROM vault_curation_events WHERE vault_id=?1 AND sequence>?2 ORDER BY sequence LIMIT ?3").map_err(db_error)?;
            let rows=q.query_map(params![vault.to_string(),after,limit],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).map_err(db_error)?;
            rows.map(|row|{let(sequence,id,at_ms,action,revision,actor,reason)=row.map_err(db_error)?;Ok(CurationEvent{sequence,document_id:id.parse().map_err(|_|rejected("invalid id"))?,at_ms,action,revision,actor,reason})}).collect()
        })
    }
    pub fn vault_provenance(
        &self,
        vault: Uuid,
        id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<ProvenanceEvent>> {
        if after < 0 || !(1..=100).contains(&limit) {
            return Err(rejected("invalid provenance page"));
        }
        self.transaction(|tx|{
            vault_exists(tx,vault)?;
            let mut q=tx.prepare("SELECT sequence,at_ms,kind,revision,path FROM vault_provenance WHERE vault_id=?1 AND document_id=?2 AND sequence>?3 ORDER BY sequence LIMIT ?4").map_err(db_error)?;
            let rows=q.query_map(params![vault.to_string(),id.to_string(),after,limit],|r|Ok(ProvenanceEvent{sequence:r.get(0)?,at_ms:r.get(1)?,kind:r.get(2)?,revision:r.get(3)?,path:r.get(4)?})).map_err(db_error)?;
            rows.map(|r|r.map_err(db_error)).collect()
        })
    }
}
#[cfg(test)]
mod tests;
