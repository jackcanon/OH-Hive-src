//! Nightly hub backups on the overlay (ADR-013 D73). Each configured HJM-operated server runs
//! independently, once per UTC day after `HIVE_BACKUP_HOUR_UTC`. A volunteer coordinator must
//! not suppress backups; redundant encrypted exports from eligible servers are intentional.
//!
//! Pipeline: `hive.backup_export()` (every table in schema hive as JSON, FK-safe order) → gzip →
//! age-encrypt to `HIVE_BACKUP_RECIPIENT` (an `age1…` public key; the private half never leaves
//! Jack's machine) → local artifact store → `hive.backup_record()` (kind='backup', pinned,
//! replication 3). Ordinary pull replication then spreads it to other servers. Restore with
//! `scripts/restore-backup.sh`. Nothing here needs the database password.

use crate::store::Store;
use anyhow::{Context, Result};
use flate2::{write::GzEncoder, Compression};
use hive_core::hub::HubClient;
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

    /// Eligibility deliberately does not depend on the general coordinator lease.
    pub fn due_for_server(&self, is_hjm: bool) -> bool {
        is_hjm && self.due()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eligible_server_backs_up_without_coordinator_and_persists_daily_limit() {
        let root = std::env::temp_dir().join(format!("hive-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("store")).unwrap();
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        let backup = Backup::from_env(&root.join("store"), Some(&recipient), 0)
            .unwrap()
            .unwrap();
        // This server need not hold any coordinator lease to be eligible.
        assert!(backup.due_for_server(true));
        assert!(!backup.due_for_server(false));
        let (day, _) = Backup::epoch_day_and_hour();
        std::fs::write(&backup.state_file, day.to_string()).unwrap();
        let restarted = Backup::from_env(&root.join("store"), Some(&recipient), 0)
            .unwrap()
            .unwrap();
        assert!(!restarted.due_for_server(true));
        std::fs::write(&backup.state_file, (day - 1).to_string()).unwrap();
        assert!(restarted.due_for_server(true));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_or_invalid_recipient_cannot_enable_backups() {
        assert!(Backup::from_env(Path::new("unused"), None, 9)
            .unwrap()
            .is_none());
        assert!(Backup::from_env(Path::new("unused"), Some(" "), 9)
            .unwrap()
            .is_none());
        assert!(Backup::from_env(Path::new("unused"), Some("invalid"), 9).is_err());
    }
}
