//! Nightly ledger archival (ADR-013 D73). Runs on the same HJM-operated, coordinator-holding
//! server as nightly backups (`backup.rs`), and reuses the same age recipient key ("hub key") for
//! confidentiality rather than standing up a second one.
//!
//! Pipeline, one calendar month per call: `hive.ledger_archive_pending()` (oldest un-archived month
//! more than 90 days old, if any) → `hive.ledger_archive_export()` (that month's rows as JSON) →
//! encode as Parquet (single row group, columnar, uncompressed — these are small monthly batches) →
//! age-encrypt to `HIVE_BACKUP_RECIPIENT` → local artifact store → `hive.ledger_archive_apply()`
//! (pins the artifact kind='ledger_archive' replication 3, checkpoints every touched account's
//! balance as of month end, deletes the now-archived hot rows — all inside one hub transaction).
//! The serve loop calls `run_one` repeatedly until it returns `Ok(None)` so a long backlog (e.g. the
//! first time this ever runs, well after the 90-day mark) drains a month at a time rather than in
//! one giant batch.

use crate::store::Store;
use anyhow::{bail, Context, Result};
use hive_core::hub::HubClient;
use parquet::{
    column::writer::ColumnWriter,
    data_type::ByteArray,
    file::{
        properties::WriterProperties,
        writer::{SerializedFileWriter, SerializedRowGroupWriter},
    },
    schema::parser::parse_message_type,
};
use std::{io::Write, str::FromStr, sync::Arc, time::Duration};

/// How often the serve loop checks whether a month is ready to archive.
pub const CHECK_EVERY: Duration = Duration::from_secs(600);

const SCHEMA: &str = "
message ledger_entry {
  REQUIRED BYTE_ARRAY id (UTF8);
  REQUIRED BYTE_ARRAY txn_id (UTF8);
  REQUIRED BYTE_ARRAY account_id (UTF8);
  REQUIRED BYTE_ARRAY entry_type (UTF8);
  REQUIRED BYTE_ARRAY direction (UTF8);
  REQUIRED BYTE_ARRAY amount_honey (UTF8);
  OPTIONAL BYTE_ARRAY rate_id (UTF8);
  OPTIONAL INT64 tokens_in;
  OPTIONAL INT64 tokens_out;
  OPTIONAL BYTE_ARRAY compute_seconds (UTF8);
  OPTIONAL BYTE_ARRAY card_id (UTF8);
  OPTIONAL BYTE_ARRAY node_id (UTF8);
  REQUIRED BYTE_ARRAY memo (UTF8);
  REQUIRED BYTE_ARRAY created_at (UTF8);
  REQUIRED BYTE_ARRAY source (UTF8);
}
";

pub struct LedgerArchiver {
    recipient: age::x25519::Recipient,
}

impl LedgerArchiver {
    /// `None` when no recipient is configured (archival off on this server — same gate as backups).
    pub fn from_env(recipient: Option<&str>) -> Result<Option<Self>> {
        let Some(r) = recipient.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let recipient = age::x25519::Recipient::from_str(r).map_err(|e| {
            anyhow::anyhow!("HIVE_BACKUP_RECIPIENT is not an age x25519 public key: {e}")
        })?;
        Ok(Some(Self { recipient }))
    }

    /// Archive the single oldest pending month, if any. Returns `None` when nothing is more than
    /// 90 days old yet (the common case for a long time). Encrypted with the same hub recipient as
    /// nightly backups (financial history, kept confidential like the rest of the ledger).
    pub async fn run_one(
        &self,
        hub: &HubClient,
        store: &Arc<Store>,
    ) -> Result<Option<(String, String, u64, usize)>> {
        let Some(month_start) = hub
            .ledger_archive_pending()
            .await
            .context("hive.ledger_archive_pending")?
        else {
            return Ok(None);
        };

        let export = hub
            .ledger_archive_export(&month_start)
            .await
            .context("hive.ledger_archive_export")?;
        let rows = export
            .get("entries")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let plain = encode_parquet(&rows)?;
        let cipher = age::encrypt(&self.recipient, &plain).context("age encrypt")?;
        let (hash, bytes) = store.put(&cipher, "application/age")?;

        hub.ledger_archive_apply(&month_start, &hash, bytes, rows.len() as u64)
            .await
            .context("hive.ledger_archive_apply")?;

        Ok(Some((month_start, hash, bytes, rows.len())))
    }
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Encode ledger_entries rows (as returned by `hive.ledger_archive_export`) as a single-row-group
/// Parquet file. `amount_honey`/`compute_seconds` are kept as exact-decimal strings rather than
/// floats — this is a financial record and every digit `hive.post_txn` wrote has to survive.
pub fn encode_parquet(rows: &[serde_json::Value]) -> Result<Vec<u8>> {
    let schema = Arc::new(parse_message_type(SCHEMA)?);
    let props = Arc::new(WriterProperties::builder().build());
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut writer = SerializedFileWriter::new(&mut buf, schema, props)?;
        let mut rg = writer.next_row_group()?;

        write_bytes_col(&mut rg, rows, "id", false)?;
        write_bytes_col(&mut rg, rows, "txn_id", false)?;
        write_bytes_col(&mut rg, rows, "account_id", false)?;
        write_bytes_col(&mut rg, rows, "entry_type", false)?;
        write_bytes_col(&mut rg, rows, "direction", false)?;
        write_bytes_col(&mut rg, rows, "amount_honey", false)?;
        write_bytes_col(&mut rg, rows, "rate_id", true)?;
        write_i64_col(&mut rg, rows, "tokens_in")?;
        write_i64_col(&mut rg, rows, "tokens_out")?;
        write_bytes_col(&mut rg, rows, "compute_seconds", true)?;
        write_bytes_col(&mut rg, rows, "card_id", true)?;
        write_bytes_col(&mut rg, rows, "node_id", true)?;
        write_bytes_col(&mut rg, rows, "memo", false)?;
        write_bytes_col(&mut rg, rows, "created_at", false)?;
        write_bytes_col(&mut rg, rows, "source", false)?;

        rg.close()?;
        writer.close()?;
    }
    Ok(buf)
}

fn write_bytes_col<W: Write + Send>(
    rg: &mut SerializedRowGroupWriter<'_, W>,
    rows: &[serde_json::Value],
    key: &str,
    nullable: bool,
) -> Result<()> {
    let mut values = Vec::with_capacity(rows.len());
    let mut def_levels = Vec::with_capacity(rows.len());
    for r in rows {
        match json_str(r, key) {
            Some(s) => {
                values.push(ByteArray::from(s.into_bytes()));
                def_levels.push(1i16);
            }
            None => {
                if !nullable {
                    bail!("required column {key} was null in an archived row");
                }
                def_levels.push(0i16);
            }
        }
    }
    let mut col = rg
        .next_column()?
        .context("parquet schema/column mismatch")?;
    match col.untyped() {
        ColumnWriter::ByteArrayColumnWriter(w) => {
            let dl = if nullable {
                Some(&def_levels[..])
            } else {
                None
            };
            w.write_batch(&values, dl, None)?;
        }
        _ => bail!("unexpected column writer type for {key}"),
    }
    col.close()?;
    Ok(())
}

fn write_i64_col<W: Write + Send>(
    rg: &mut SerializedRowGroupWriter<'_, W>,
    rows: &[serde_json::Value],
    key: &str,
) -> Result<()> {
    let mut values = Vec::with_capacity(rows.len());
    let mut def_levels = Vec::with_capacity(rows.len());
    for r in rows {
        match r.get(key).and_then(|v| v.as_i64()) {
            Some(n) => {
                values.push(n);
                def_levels.push(1i16);
            }
            None => def_levels.push(0i16),
        }
    }
    let mut col = rg
        .next_column()?
        .context("parquet schema/column mismatch")?;
    match col.untyped() {
        ColumnWriter::Int64ColumnWriter(w) => {
            w.write_batch(&values, Some(&def_levels), None)?;
        }
        _ => bail!("unexpected column writer type for {key}"),
    }
    col.close()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::{
        file::reader::{FileReader, SerializedFileReader},
        record::Field,
    };
    use std::collections::HashMap;

    fn as_map(row: &parquet::record::Row) -> HashMap<String, Field> {
        row.get_column_iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    #[test]
    fn round_trips_a_balanced_txn() {
        let rows: Vec<serde_json::Value> = vec![
            serde_json::json!({
                "id": "11111111-1111-1111-1111-111111111111",
                "txn_id": "22222222-2222-2222-2222-222222222222",
                "account_id": "33333333-3333-3333-3333-333333333333",
                "entry_type": "adjustment", "direction": "debit", "amount_honey": "12.500000",
                "rate_id": null, "tokens_in": null, "tokens_out": null, "compute_seconds": null,
                "card_id": null, "node_id": null, "memo": "test debit",
                "created_at": "2026-01-15T00:00:00+00:00", "source": "grant",
            }),
            serde_json::json!({
                "id": "44444444-4444-4444-4444-444444444444",
                "txn_id": "22222222-2222-2222-2222-222222222222",
                "account_id": "55555555-5555-5555-5555-555555555555",
                "entry_type": "adjustment", "direction": "credit", "amount_honey": "12.500000",
                "rate_id": null, "tokens_in": 100, "tokens_out": 200, "compute_seconds": "1.500",
                "card_id": null, "node_id": null, "memo": "test credit",
                "created_at": "2026-01-15T00:00:00+00:00", "source": "grant",
            }),
        ];

        let bytes = encode_parquet(&rows).expect("encode");
        assert!(!bytes.is_empty());

        let reader = SerializedFileReader::new(bytes::Bytes::from(bytes)).expect("open parquet");
        assert_eq!(reader.metadata().file_metadata().num_rows(), 2);

        // round-trip through the row iterator to check the exact-decimal strings survived.
        let mut iter = reader.get_row_iter(None).expect("row iter");
        let row0 = as_map(&iter.next().unwrap().expect("row 0"));
        assert_eq!(row0["amount_honey"], Field::Str("12.500000".to_string()));
        assert_eq!(row0["memo"], Field::Str("test debit".to_string()));
        assert_eq!(row0["rate_id"], Field::Null);

        let row1 = as_map(&iter.next().unwrap().expect("row 1"));
        assert_eq!(row1["tokens_in"], Field::Long(100));
        assert_eq!(row1["compute_seconds"], Field::Str("1.500".to_string()));
        assert!(iter.next().is_none());
    }
}
