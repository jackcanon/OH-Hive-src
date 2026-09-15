//! Host-owned maintenance future. Persistent cadence; no new daemon, UI or remote writes.
use super::*;
use std::time::Duration;
const DAY: i64 = 86_400_000;
const CAP: i64 = 64 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenancePolicy {
    pub enabled: bool,
    pub interval_seconds: u32,
    pub stale_after_days: u32,
    /// None retains all snapshots. Some opts into discarding aged redundant copies.
    pub redundant_snapshot_days: Option<u32>,
    pub archive_quota_bytes: i64,
}
impl Default for MaintenancePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_seconds: 86400,
            stale_after_days: 90,
            redundant_snapshot_days: None,
            archive_quota_bytes: CAP,
        }
    }
}
impl MaintenancePolicy {
    fn validate(&self) -> Result<()> {
        if !(60..=2_592_000).contains(&self.interval_seconds)
            || !(1..=3650).contains(&self.stale_after_days)
            || self
                .redundant_snapshot_days
                .is_some_and(|n| !(1..=3650).contains(&n))
            || !(1024..=CAP).contains(&self.archive_quota_bytes)
        {
            return Err(rejected("invalid maintenance policy"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceResult {
    pub vault_id: Uuid,
    pub finished_ms: i64,
    pub outcome: String,
    pub notes: usize,
    pub stale: usize,
    pub duplicates: usize,
    pub duplicate_scan_complete: bool,
    pub snapshots_expired: usize,
    pub stale_candidates: Vec<Uuid>,
    pub duplicate_candidates: Vec<super::vault_curation::DuplicateCandidate>,
    pub findings_truncated: bool,
    pub archive_bytes: i64,
    pub over_quota: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceStatus {
    pub policy: MaintenancePolicy,
    pub next_due_ms: i64,
    /// Stored claim exists; may be abandoned until the one-hour retry lease expires.
    pub running: bool,
    pub last_result: Option<MaintenanceResult>,
}
fn policy(tx: &Transaction<'_>, v: Uuid) -> Result<MaintenancePolicy> {
    let p: Option<String> = tx
        .query_row(
            "SELECT policy FROM vault_maintenance WHERE vault_id=?1",
            [v.to_string()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    p.map(|s| decode(&s))
        .transpose()
        .map(|p| p.unwrap_or_default())
}
fn bytes(tx: &Transaction<'_>, v: Uuid) -> Result<i64> {
    tx.query_row("SELECT coalesce(sum(length(CAST(snapshot AS BLOB))),0) FROM vault_archives WHERE vault_id=?1",[v.to_string()],|r|r.get(0)).map_err(db_error)
}
fn expire(tx: &Transaction<'_>, v: Uuid, p: &MaintenancePolicy, at: i64) -> Result<usize> {
    let ready: bool = tx
        .query_row(
            "SELECT state='ready' FROM vaults WHERE id=?1",
            [v.to_string()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !ready {
        return Ok(0);
    }
    let Some(days) = p.redundant_snapshot_days else {
        return Ok(0);
    };
    // Exact current serialized copy must still exist; unique historical snapshots are protected.
    let mut q=tx.prepare("SELECT a.document_id,a.revision,a.snapshot,d.path,d.title,d.content FROM vault_archives a JOIN vault_documents d ON d.id=a.document_id AND d.vault_id=a.vault_id AND d.revision=a.revision WHERE a.vault_id=?1 AND a.snapshot IS NOT NULL AND a.archived_ms<=?2 ORDER BY a.archived_ms,a.document_id LIMIT 1000").map_err(db_error)?;
    let rows = q
        .query_map(
            params![v.to_string(), at.saturating_sub(i64::from(days) * DAY)],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(db_error)?;
    let rows = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_error)?;
    let mut count = 0;
    for (id, revision, snapshot, path, title, content) in rows {
        let doc = super::vault::VaultDocument {
            id: id
                .parse()
                .map_err(|_| rejected("invalid archive identity"))?,
            vault_id: v,
            path,
            revision: revision.clone(),
            title,
            content,
        };
        if encode(&doc)? != snapshot {
            continue;
        }
        tx.execute(
            "UPDATE vault_archives SET snapshot=NULL WHERE document_id=?1 AND vault_id=?2",
            params![id, v.to_string()],
        )
        .map_err(db_error)?;
        tx.execute("INSERT INTO vault_curation_events(vault_id,document_id,at_ms,action,revision,actor,reason) VALUES(?1,?2,?3,'expire_snapshot',?4,'host-maintenance','opt-in retention: identical indexed copy retained; archive overlay preserved')",params![v.to_string(),id,at,revision]).map_err(db_error)?;
        count += 1;
    }
    Ok(count)
}
pub(super) fn reserve_archive(tx: &Transaction<'_>, v: Uuid, incoming: i64, at: i64) -> Result<()> {
    let p = policy(tx, v)?;
    if incoming > p.archive_quota_bytes {
        return Err(rejected("snapshot exceeds configured archive quota"));
    }
    if bytes(tx, v)?.saturating_add(incoming) > p.archive_quota_bytes {
        expire(tx, v, &p, at)?;
    }
    if bytes(tx, v)?.saturating_add(incoming) > p.archive_quota_bytes {
        return Err(rejected("archive quota full; protected snapshots retained"));
    }
    Ok(())
}
impl LocalHubStore {
    /// Config changes cancel publication by any in-flight run. Does not start a background task.
    pub fn vault_configure_maintenance(&self, v: Uuid, p: &MaintenancePolicy) -> Result<()> {
        p.validate()?;
        self.transaction(|tx|{
            tx.execute("INSERT INTO vault_maintenance(vault_id,policy,next_due_ms) VALUES(?1,?2,?3) ON CONFLICT(vault_id) DO UPDATE SET policy=excluded.policy,next_due_ms=excluded.next_due_ms,token=NULL,lease_until_ms=0",params![v.to_string(),encode(p)?,Utc::now().timestamp_millis()]).map_err(db_error)?;Ok(())
        })
    }
    pub fn vault_maintenance_status(&self, v: Uuid) -> Result<Option<MaintenanceStatus>> {
        self.transaction(|tx|{
            let row:Option<(String,i64,Option<String>,Option<String>)>=tx.query_row("SELECT policy,next_due_ms,token,last_result FROM vault_maintenance WHERE vault_id=?1",[v.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(db_error)?;
            row.map(|(p,next_due_ms,token,result)|Ok(MaintenanceStatus{policy:decode(&p)?,next_due_ms,running:token.is_some(),last_result:result.map(|s|decode(&s)).transpose()?})).transpose()
        })
    }
    /// One due vault per tick for fairness. Claims persist; abandoned runs retry after one hour.
    /// Summaries are observations, not revision-bound approvals. No automatic note mutations.
    pub fn vault_maintenance_tick(&self) -> Result<Option<MaintenanceResult>> {
        self.maintenance_tick_at(Utc::now().timestamp_millis())
    }
    fn maintenance_tick_at(&self, at: i64) -> Result<Option<MaintenanceResult>> {
        let claim=self.transaction(|tx|{
            let mut q=tx.prepare("SELECT m.vault_id,m.policy FROM vault_maintenance m WHERE m.next_due_ms<=?1 AND (m.token IS NULL OR m.lease_until_ms<=?1) ORDER BY m.next_due_ms,m.vault_id").map_err(db_error)?;
            let rows=q.query_map([at],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).map_err(db_error)?;
            for row in rows {
                let(id,raw)=row.map_err(db_error)?;let p:MaintenancePolicy=decode(&raw)?;
                if !p.enabled {continue;}
                p.validate()?;
                let token=Uuid::new_v4().to_string();
                tx.execute("UPDATE vault_maintenance SET token=?2,lease_until_ms=?3 WHERE vault_id=?1",params![id,token,at.saturating_add(3_600_000)]).map_err(db_error)?;
                return Ok(Some((id.parse::<Uuid>().map_err(|_|rejected("invalid vault id"))?,p,token)));
            }
            Ok(None)
        })?;
        let Some((v, p, token)) = claim else {
            return Ok(None);
        };
        let ready = self.transaction(|tx| {
            tx.query_row(
                "SELECT state='ready' FROM vaults WHERE id=?1",
                [v.to_string()],
                |r| r.get::<_, bool>(0),
            )
            .map_err(db_error)
        })?;
        let scan = if ready {
            Some(self.vault_curation_scan(v, p.stale_after_days))
        } else {
            None
        };
        self.transaction(|tx|{
            let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM vault_maintenance WHERE vault_id=?1 AND token=?2)",params![v.to_string(),token],|r|r.get(0)).map_err(db_error)?;
            if !valid {return Ok(None);}
            let mut result=MaintenanceResult{vault_id:v,finished_ms:Utc::now().timestamp_millis(),outcome:"unavailable".into(),notes:0,stale:0,duplicates:0,duplicate_scan_complete:false,snapshots_expired:0,stale_candidates:Vec::new(),duplicate_candidates:Vec::new(),findings_truncated:false,archive_bytes:bytes(tx,v)?,over_quota:false};
            match scan {
                Some(Ok(report)) if report.vault_state=="ready"=>{
                    result.outcome="scanned".into();result.notes=report.notes.len();result.stale=report.stale.len();result.duplicates=report.duplicates.len();result.duplicate_scan_complete=report.duplicate_scan_complete;
                    result.findings_truncated=report.stale.len()>1000 || !report.duplicate_scan_complete;
                    result.stale_candidates=report.stale.into_iter().take(1000).collect();
                    result.duplicate_candidates=report.duplicates;
                    result.snapshots_expired=expire(tx,v,&p,at)?;
                },
                Some(Err(_))=>result.outcome="scan_failed".into(),
                _=>{},
            }
            result.archive_bytes=bytes(tx,v)?;result.over_quota=result.archive_bytes>p.archive_quota_bytes;
            let delay=if result.outcome=="scanned" {i64::from(p.interval_seconds)*1000} else {60_000};
            tx.execute("UPDATE vault_maintenance SET token=NULL,lease_until_ms=0,next_due_ms=?2,last_finished_ms=?3,last_result=?4 WHERE vault_id=?1",params![v.to_string(),at.saturating_add(delay),result.finished_ms,encode(&result)?]).map_err(db_error)?;
            Ok(Some(result))
        })
    }
    /// Host owns this future. Work is offloaded from async executor; shutdown waits for current
    /// bounded scan, then stops. Dropping the future does not cancel an already started scan.
    pub async fn vault_maintenance_run(
        &self,
        mut stop: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        loop {
            if *stop.borrow() {
                return Ok(());
            }
            let store = self.clone();
            tokio::task::spawn_blocking(move || store.vault_maintenance_tick())
                .await
                .map_err(|_| rejected("maintenance task failed"))??;
            tokio::select! {
                _=tokio::time::sleep(Duration::from_secs(30))=>{},
                changed=stop.changed()=>{if changed.is_err()||*stop.borrow(){return Ok(())}},
            }
        }
    }
}
#[cfg(test)]
mod tests;
