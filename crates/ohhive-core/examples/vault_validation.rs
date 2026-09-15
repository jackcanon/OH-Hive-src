//! Synthetic-only local vault validation harness. Administration is stdin, never network RPC.
use hive_core::local_hub::{serve, LocalHubStore};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Write},
    time::Instant,
};
use uuid::Uuid;
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("hive-vault-validation-{}", Uuid::new_v4()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn emit(v: Value) {
    println!("{v}");
    std::io::stdout().flush().unwrap();
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let fixture = Fixture::new();
    let root = fixture.0.join("notes");
    std::fs::create_dir(&root)?;
    let s = LocalHubStore::open(fixture.0.join("hub.sqlite"))?;
    let v = s.vault_create("Synthetic acceptance notes")?;
    s.vault_attach_folder(v, &root)?;
    if a.first().is_some_and(|s| s == "scale") {
        let c = s.enroll_owner("benchmark")?;
        s.vault_grant(v, c.node_id, true)?;
        let reader = s.connect(&c.raw_key)?;
        let content = format!("# Fixture\nneedle {}", "word ".repeat(200));
        for count in [100, 1000, 10_000] {
            for i in 0..count {
                std::fs::write(root.join(format!("{i}.md")), &content)?;
            }
            let mut scans = Vec::new();
            let mut searches = Vec::new();
            for _ in 0..3 {
                let t = Instant::now();
                s.vault_scan_folder(v)?;
                scans.push(t.elapsed().as_secs_f64() * 1000.);
                let t = Instant::now();
                assert_eq!(reader.vault_search(v, "needle", 10)?.len(), 10);
                searches.push(t.elapsed().as_secs_f64() * 1000.);
            }
            emit(
                json!({"notes":count,"text_bytes":count*content.len(),"scan_ms":scans,"search_ms":searches,"build":if cfg!(debug_assertions){"debug"}else{"release"},"db_bytes":std::fs::metadata(fixture.0.join("hub.sqlite"))?.len()}),
            );
        }
        return Ok(());
    }
    if a.first()
        .is_some_and(|s| s == "near-limit" || s == "near-limit-edit")
    {
        near_limit(&s, v, &root, a[0] == "near-limit-edit")?;
        return Ok(());
    }
    std::fs::write(root.join("a.md"), "# First\nacceptance alpha")?;
    s.vault_scan_folder(v)?;
    let listener = tokio::net::TcpListener::bind(
        a.first()
            .ok_or_else(|| anyhow::anyhow!("bind address or scale required"))?,
    )
    .await?;
    let addr = listener.local_addr()?;
    let (stop, rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve(s.clone(), listener, async {
        let _ = rx.await;
    }));
    emit(json!({"origin":format!("http://{addr}"),"vault":v,"pair_code":s.pairing_code()?}));
    for line in std::io::stdin().lock().lines() {
        let cmd: Value = serde_json::from_str(&line?)?;
        let result: Value = match cmd["op"].as_str().unwrap_or("") {
            "grant" => {
                s.vault_grant(
                    v,
                    cmd["node"].as_str().unwrap().parse()?,
                    cmd["enabled"].as_bool().unwrap(),
                )?;
                json!({"ok":true})
            }
            "revoke" => {
                s.revoke(cmd["node"].as_str().unwrap().parse()?)?;
                json!({"ok":true})
            }
            "edit" => {
                std::fs::rename(root.join("a.md"), root.join("renamed.md"))?;
                s.vault_scan_folder(v)?;
                json!({"ok":true})
            }
            "missing" => {
                std::fs::rename(&root, fixture.0.join("away"))?;
                assert!(s.vault_scan_folder(v).is_err());
                json!({"ok":true})
            }
            "restore" => {
                std::fs::rename(fixture.0.join("away"), &root)?;
                s.vault_scan_folder(v)?;
                json!({"ok":true})
            }
            "stop" => break,
            _ => json!({"error":"unknown operation"}),
        };
        emit(result);
    }
    let _ = stop.send(());
    server.await??;
    emit(json!({"stopped":true}));
    Ok(())
}

fn near_limit(
    s: &LocalHubStore,
    v: Uuid,
    root: &std::path::Path,
    edit: bool,
) -> anyhow::Result<()> {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    };
    // Generated project notes with varied titles, task records and vocabulary; no personal data.
    let topics = [
        "database",
        "garden",
        "network",
        "design",
        "testing",
        "storage",
        "accessibility",
        "meeting",
    ];
    let mut bytes = 0usize;
    let mut count = 0;
    while bytes < 63 * 1024 * 1024 {
        let mut note = format!("# Project notebook {}\n\n## Review agenda\n", count);
        for section in 0..110 {
            let topic = topics[(count + section) % topics.len()];
            note.push_str(&format!("### {topic} observation {section}\nThe team reviewed item {} for project {}. We recorded the current behavior, a proposed improvement, and a validation step. The owner will compare results before changing the plan.\n- Status: review pending\n- Next step: verify {topic} requirements with the assigned maintainer.\n\n",count*110+section,count));
        }
        bytes += note.len();
        std::fs::write(root.join(format!("{count}.md")), note)?;
        count += 1;
    }
    assert!(bytes <= 64 * 1024 * 1024);
    let c = s.enroll_owner("contention benchmark")?;
    s.vault_grant(v, c.node_id, true)?;
    let reader = s.connect(&c.raw_key)?;
    let mut scans = Vec::new();
    let mut publications = Vec::new();
    for round in 0..3 {
        if edit {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(root.join("0.md"))?,
                "Update {round}"
            )?;
        }
        let t = Instant::now();
        let status = s.vault_scan_folder(v)?;
        scans.push(t.elapsed().as_secs_f64() * 1000.);
        publications.push(status.publication_transaction_ms);
    }
    let mut baseline = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        assert_eq!(reader.vault_search(v, "database", 10)?.len(), 10);
        baseline.push(t.elapsed().as_secs_f64() * 1000.);
    }
    let mut trials = Vec::new();
    for round in 0..3 {
        if edit {
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(root.join("0.md"))?,
                "Concurrent update {round}"
            )?;
        }
        let done = Arc::new(AtomicBool::new(false));
        let start = Arc::new(Barrier::new(2));
        let d = done.clone();
        let b = start.clone();
        let reader = reader.clone();
        let thread = std::thread::spawn(move || {
            let mut reads = Vec::new();
            b.wait();
            while !d.load(Ordering::Acquire) {
                let t = Instant::now();
                let ok = reader
                    .vault_search(v, "database", 10)
                    .is_ok_and(|h| h.len() == 10);
                reads.push(json!({"ms":t.elapsed().as_secs_f64()*1000.,"ok":ok}));
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            reads
        });
        start.wait();
        let t = Instant::now();
        let result = s.vault_scan_folder(v);
        let elapsed = t.elapsed().as_secs_f64() * 1000.;
        done.store(true, Ordering::Release);
        let reads = thread.join().unwrap();
        let status = result?;
        trials.push(json!({"scan_ms":elapsed,"publication_transaction_ms":status.publication_transaction_ms,"reads":reads}));
    }
    emit(
        json!({"build":if cfg!(debug_assertions){"debug"}else{"release"},"mutation":if edit {"one_note_each_scan"} else {"none"},"notes":count,"text_bytes":bytes,"scan_ms":scans,"publication_transaction_ms":publications,"baseline_search_ms":baseline,"concurrent_trials":trials}),
    );
    Ok(())
}
