//! Opt-in live smoke: GitHub token arrives on stdin, never arguments or saved task data.
use hive_core::{
    backend::{llama_cpp::LlamaCppBackend, Backend},
    capability::ToolsLevel,
    hub::Hub,
    local_hub::{
        private_code_tasks::PrivateCodeTaskRequest, repository::ProjectRepository, LocalHubStore,
    },
    worker::Worker,
};
use std::{io::Read, path::PathBuf, time::Duration};
use uuid::Uuid;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() == 4, "usage: private_task_smoke <new-data-dir> <github-https-repo> <local-model-url> <model-id>; token on stdin");
    let data = PathBuf::from(&args[0]);
    std::fs::create_dir(&data)?;
    let mut token = String::new();
    std::io::stdin().take(4097).read_to_string(&mut token)?;
    let store = LocalHubStore::open(data.join("smoke.sqlite"))?;
    let creds = store.enroll_owner("isolated live smoke")?;
    let project =
        store.create_project("Private task smoke", "Create one test file; no publication")?;
    store.set_project_repository(
        project,
        Some(&ProjectRepository {
            repo_url: args[1].clone(),
            repo_ref: None,
        }),
    )?;
    let task = Uuid::new_v4();
    store.stage_private_code_task(&PrivateCodeTaskRequest {
        request_id:task, project_id:project, target_node_id:creds.node_id,
        title:"Create smoke marker".into(),
        task:"Use write_file to create hive-smoke.txt containing exactly: Hive private task smoke passed\nThen finish. Do not run commands, change any other files, commit, push, or access the network.".into(),
        model_id:Some(args[3].clone()), max_turns:6, acceptance:vec![],
    })?;
    let root = store
        .prepare_private_code_task(task, creds.node_id, &data, token.trim())
        .await?;
    drop(token);
    println!("Authenticated checkout prepared; task {task}");
    let hub = store.connect(&creds.raw_key)?.restricted_to_card(task);
    let backend = LlamaCppBackend::new(&args[2]);
    let mut caps = backend.capabilities().await?;
    caps.tools_level = ToolsLevel::SandboxedTools;
    caps.allow_internet = false;
    hub.check_in(&caps, None).await?;
    let (stop, rx) = tokio::sync::watch::channel(false);
    let watchdog = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(180)).await;
        let _ = stop.send(true);
    });
    let worker = Worker {
        hub: &hub,
        backend: &backend,
        caps: &caps,
        default_model: Some(args[3].clone()),
        stop: rx,
        events: None,
        data_dir: data.clone(),
        sandbox: None,
    };
    let result = worker.tick_with_heartbeat().await;
    watchdog.abort();
    hub.check_out().await?;
    anyhow::ensure!(
        result?,
        "Task did not run; execution slot or model eligibility unavailable"
    );
    let statuses = store.private_code_task_statuses(project, creds.node_id)?;
    anyhow::ensure!(
        statuses.len() == 1 && statuses[0].status == "review",
        "Task did not reach review"
    );
    anyhow::ensure!(
        std::fs::read_to_string(root.join("hive-smoke.txt"))?.trim()
            == "Hive private task smoke passed",
        "Marker content mismatch"
    );
    println!(
        "PASS: private task reached review and produced the expected file. Workspace: {}",
        root.display()
    );
    Ok(())
}
