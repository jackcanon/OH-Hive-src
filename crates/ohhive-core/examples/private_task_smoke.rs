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
    let retry_check = args.get(4).is_some_and(|s| s == "--retry-check");
    anyhow::ensure!(args.len() == 4 || (args.len() == 5 && retry_check), "usage: private_task_smoke <new-data-dir> <github-https-repo> <local-model-url> <model-id> [--retry-check]; token on stdin");
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
        model_id:Some(args[3].clone()), max_turns:6, acceptance:if retry_check { vec![hive_core::acceptance::AcceptanceCheck {
            name:"Owner fixture gate".into(), command:"/bin/test".into(), args:vec!["-f".into(), "smoke-owner-ready".into()], cwd:None, expect_exit:0, required:true,
        }] } else { vec![] },
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
    let attempts = if retry_check { 2 } else { 1 };
    for attempt in 0..attempts {
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
        anyhow::ensure!(statuses.len() == 1, "Missing task status");
        anyhow::ensure!(
            std::fs::read_to_string(root.join("hive-smoke.txt"))?.trim()
                == "Hive private task smoke passed",
            "Marker content mismatch"
        );
        if retry_check && attempt == 0 {
            anyhow::ensure!(
                statuses[0].status == "blocked",
                "Failed acceptance did not block the task"
            );
            anyhow::ensure!(
                !root.join("smoke-owner-ready").exists(),
                "Agent changed the owner gate"
            );
            let reason = statuses[0].reason.as_deref().unwrap_or("");
            let receipt: serde_json::Value = serde_json::from_str(
                reason
                    .rsplit_once("Acceptance checks: ")
                    .map(|(_, r)| r)
                    .unwrap_or("null"),
            )?;
            anyhow::ensure!(
                receipt["status"] == "failed" && receipt["results"][0]["exit_status"] == 1,
                "First attempt did not record a failed command exit"
            );
            println!(
                "PASS: required check blocked the first attempt while preserving model output"
            );
            std::fs::write(
                root.join("smoke-owner-ready"),
                "owner corrected test prerequisite\n",
            )?;
            let marker = std::fs::read(root.join("hive-smoke.txt"))?;
            store
                .retry_private_code_task(task, creds.node_id, &data)
                .await?;
            let ready = store.private_code_task_statuses(project, creds.node_id)?;
            anyhow::ensure!(
                ready[0].status == "ready" && ready[0].check_count == 1,
                "Recovery lost the check"
            );
            anyhow::ensure!(
                std::fs::read(root.join("hive-smoke.txt"))? == marker,
                "Recovery changed the file"
            );
            println!("PASS: explicit retry retained files and the required check");
        } else {
            anyhow::ensure!(statuses[0].status == "review", "Task did not reach review");
            if retry_check {
                let output = statuses[0].output.as_deref().unwrap_or("");
                let receipt: serde_json::Value = serde_json::from_str(
                    output
                        .rsplit_once("Acceptance checks: ")
                        .map(|(_, r)| r)
                        .unwrap_or("null"),
                )?;
                anyhow::ensure!(
                    receipt["status"] == "passed" && receipt["results"][0]["exit_status"] == 0,
                    "Retry did not record passed acceptance"
                );
            }
        }
    }
    println!(
        "PASS: private task reached review and produced the expected file. Workspace: {}",
        root.display()
    );
    Ok(())
}
