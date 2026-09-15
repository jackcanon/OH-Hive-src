//! Owner-local intake adapter: one explicitly approved JSON item on stdin, receipt on stdout.
use hive_core::local_hub::{vault_intake::IntakeItem, LocalHubStore};
use std::io::Read;
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 2,
        "usage: vault_intake <hub-db> <vault-id|new>"
    );
    // Bound the wire input before decoding (JSON escaping can expand a 1 MiB source).
    let mut input = String::new();
    std::io::stdin()
        .take(7 * 1024 * 1024 + 1)
        .read_to_string(&mut input)?;
    anyhow::ensure!(input.len() <= 7 * 1024 * 1024, "intake payload too large");
    let item: IntakeItem = serde_json::from_str(&input)?;
    let store = LocalHubStore::open(&args[0])?;
    let vault = if args[1] == "new" {
        store.vault_create("Managed intake")?
    } else {
        args[1].parse()?
    };
    let receipt = store.vault_intake(vault, &item)?;
    store.vault_reopen_manual()?;
    println!(
        "{}",
        serde_json::json!({"vault_id":vault,"receipt":receipt})
    );
    Ok(())
}
