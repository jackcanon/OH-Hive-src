//! Nightly hub backups on the overlay (ADR-013 D73). Runs only on an HJM-operated server that
//! currently holds the coordinator lease, once per UTC day after `HIVE_BACKUP_HOUR_UTC`.
//!
//! Pipeline: `hive.backup_export()` (every table in schema hive as JSON, FK-safe order) → gzip →
//! age-encrypt to `HIVE_BACKUP_RECIPIENT` (an `age1…` public key; the private half never leaves
//! Jack's machine) → local artifact store → `hive.backup_record()` (kind='backup', pinned,
//! replication 3). Ordinary pull replication then spreads it to other servers. Restore with
//! `scripts/restore-backup.sh`. Nothing here needs the database password.

use crate::store::Store;
use anyhow::{Context, Result};
use flate2::{write::GzEncoder, Compression};
use ohhive_core::hub::HubClient;
use std::{
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How often the serve loop checks whether a backup is due.
pub const CHECK_EVERY: Duration = Duration::from_secs(600);

pub struct Backup {
    recipient: age::x25519::Recipient,
    hour_utc: u32,
    state_file: PathBuf,
}

impl Backup {
    /// `None` when no recipient is configured (backups off on this server).
    pub fn from_env(
        data_dir: &Path,
        recipient: Option<&str>,
        hour_utc: u32,
    ) -> Result<Option<Self>> {
        let Some(r) = recipient.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let recipient = age::x25519::Recipient::from_str(r).map_err(|e| {
            anyhow::anyhow!("HIVE_BACKUP_RECIPIENT is not an age x25519 public key: {e}")
        })?;
        Ok(Some(Self {
            recipient,
            hour_utc: hour_utc.min(23),
            state_file: data_dir.join("..").join("backup.state"),
        }))
    }

    pub fn hour_utc(&self) -> u32 {
        self.hour_utc
    }

    fn epoch_day_and_hour() -> (u64, u32) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        (secs / 86_400, ((secs % 86_400) / 3_600) as u32)
    }

    fn last_day(&self) -> Option<u64> {
        std::fs::read_to_string(&self.state_file)
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Due once per UTC day, at or after `hour_utc`.
    pub fn due(&self) -> bool {
        let (day, hour) = Self::epoch_day_and_hour();
        hour >= self.hour_utc && self.last_day() != Some(day)
    }

    /// Export → gzip → age → store → record. Returns (hash, ciphertext bytes, plaintext bytes).
    pub async fn run(&self, hub: &HubClient, store: &Arc<Store>) -> Result<(String, u64, usize)> {
        let doc = hub.backup_export().await.context("hive.backup_export")?;
        let plain = serde_json::to_vec(&doc)?;
        let mut gz = GzEncoder::new(Vec::with_capacity(plain.len() / 4), Compression::default());
        gz.write_all(&plain)?;
        let gz = gz.finish()?;
        let cipher = age::encrypt(&self.recipient, &gz).context("age encrypt")?;
        let (hash, bytes) = store.put(&cipher, "application/age")?;
        hub.backup_record(&hash, bytes)
            .await
            .context("hive.backup_record")?;
        let (day, _) = Self::epoch_day_and_hour();
        let _ = std::fs::write(&self.state_file, day.to_string());
        Ok((hash, bytes, plain.len()))
    }
}
