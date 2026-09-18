//! Explicit one-job private execution. Never constructs a community HubClient.
use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::{
    backend::Backend,
    hub::Hub,
    local_hub::{private_code_tasks::PrivateCodeTaskRequest, LocalHubStore},
};
use std::sync::Arc;
use uuid::Uuid;
fn fail(s: &str) -> HiveError {
    HiveError::Failed(s.into())
}
fn id(s: &str) -> Result<Uuid, HiveError> {
    Uuid::parse_str(s).map_err(|_| fail("Invalid task or project identity"))
}

/// User-authored required checks; arguments remain distinct (no shell parsing).
#[derive(Clone, uniffi::Record)]
pub struct PrivateTaskCheck {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
}
pub(crate) fn acceptance_checks(
    checks: Vec<PrivateTaskCheck>,
) -> Result<Vec<hive_core::acceptance::AcceptanceCheck>, HiveError> {
    let checks: Vec<_> = checks
        .into_iter()
        .map(|c| hive_core::acceptance::AcceptanceCheck {
            name: c.name,
            command: c.command,
            args: c.args,
            cwd: None,
            expect_exit: 0,
            required: true,
        })
        .collect();
    hive_core::acceptance::validate(&checks).map_err(fail)?;
    Ok(checks)
}

#[derive(Clone, uniffi::Record)]
pub struct PrivateJobStatus {
    pub id: String,
    pub title: String,
    pub status: String,
    pub reason: Option<String>,
    pub workspace: Option<String>,
    pub output: Option<String>,
    pub check_count: u32,
}
/// A model on the configured execution server; no inference is performed by discovery.
#[derive(Clone, uniffi::Record)]
pub struct PrivateCodingModel {
    pub id: String,
    pub supports_tools: Option<bool>,
}
#[derive(Clone, uniffi::Record)]
pub struct PrivateCodingHost {
    pub node_id: String,
    pub name: String,
    pub fresh: bool,
    pub observed_at: Option<i64>,
    pub worker_enabled: bool,
    pub coding_enabled: bool,
    pub git_connected: bool,
    pub models: Vec<PrivateCodingModel>,
}
impl HiveNode {
    pub(crate) fn private_job_context(&self) -> Result<(LocalHubStore, Uuid, String), HiveError> {
        if crate::private_fleet::selected()?.is_some() {
            return Err(fail(
                "Manage and run these tasks on your selected primary computer.",
            ));
        }
        self.private_bots_context()?
            .map(|(s, _, node, key)| (s, node, key))
            .ok_or_else(|| fail("Sign in to Private Fleet first"))
    }
}

#[uniffi::export]
impl HiveNode {
    /// Enrolled hosts and expiring self-reported metadata, not a promise of successful execution.
    pub async fn private_coding_hosts(self: Arc<Self>) -> Result<Vec<PrivateCodingHost>, HiveError> {
        RUNTIME.spawn_blocking(move || {
            let (store, _, key) = self.private_job_context()?;
            let hub=store.connect(&key).map_err(HiveError::from)?;
            Ok(hub.private_coding_hosts().map_err(HiveError::from)?.into_iter().map(|host| {
                let report=host.report;
                PrivateCodingHost {
                    node_id:host.host.node_id.to_string(),name:host.host.name,fresh:host.fresh,observed_at:host.observed_at,
                    worker_enabled:report.as_ref().is_some_and(|r|r.worker_enabled),
                    coding_enabled:report.as_ref().is_some_and(|r|r.coding_enabled),
                    git_connected:report.as_ref().is_some_and(|r|r.git_connected),
                    models:report.map(|r|r.models.into_iter().map(|m|PrivateCodingModel { id:m.id,supports_tools:m.supports_tools }).collect()).unwrap_or_default(),
                }
            }).collect())
        }).await.map_err(|_|fail("Execution host discovery stopped"))?
    }

    pub async fn private_coding_models(
        self: Arc<Self>,
    ) -> Result<Vec<PrivateCodingModel>, HiveError> {
        RUNTIME
            .spawn(async move {
                let _gate = self
                    .fleet
                    .gate
                    .try_lock()
                    .map_err(|_| fail("Another private operation is active"))?;
                self.private_job_context()?;
                let cfg = hive_core::nodeconfig::load().map_err(HiveError::from)?;
                let backend = hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                let caps = backend
                    .capabilities()
                    .await
                    .map_err(|_| fail("Cannot reach the configured model server"))?;
                let mut models = Vec::new();
                for model in caps.models.iter().take(64) {
                    let support = backend.model_tool_support(&model.id).await.map_err(|_| {
                        fail("Cannot check model tool support. Try refreshing models.")
                    })?;
                    models.push(PrivateCodingModel {
                        id: model.id.clone(),
                        supports_tools: support,
                    });
                }
                Ok(models)
            })
            .await
            .map_err(|_| fail("Model discovery stopped"))?
    }

    pub async fn private_jobs(
        self: Arc<Self>,
        project_id: String,
    ) -> Result<Vec<PrivateJobStatus>, HiveError> {
        // Read-only polling stays available while a job owns the primary-selection gate.
        RUNTIME
            .spawn_blocking(move || {
                let (store, node, _) = self.private_job_context()?;
                Ok(store
                    .private_code_task_statuses(id(&project_id)?, node)
                    .map_err(HiveError::from)?
                    .into_iter()
                    .map(|s| PrivateJobStatus {
                        id: s.id.to_string(),
                        title: s.title,
                        status: s.status,
                        reason: s.reason,
                        workspace: s.workspace,
                        output: s.output,
                        check_count: s.check_count,
                    })
                    .collect())
            })
            .await
            .map_err(|_| fail("Task status loading stopped"))?
    }

    // Keep the existing native submission signature; checks are an additive typed field.
    #[allow(clippy::too_many_arguments)]
    pub async fn private_job_stage(
        self: Arc<Self>,
        request_id: String,
        project_id: String,
        title: String,
        task: String,
        model_id: Option<String>,
        max_turns: u32,
        checks: Vec<PrivateTaskCheck>,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _gate = self.fleet.gate.lock().await;
                let node = self.clone();
                RUNTIME
                    .spawn_blocking(move || {
                        let (store, target, _) = node.private_job_context()?;
                        store
                            .stage_private_code_task(&PrivateCodeTaskRequest {
                                request_id: id(&request_id)?,
                                project_id: id(&project_id)?,
                                target_node_id: target,
                                title,
                                task,
                                model_id,
                                max_turns,
                                acceptance: acceptance_checks(checks)?,
                            })
                            .map_err(HiveError::from)?;
                        Ok(())
                    })
                    .await
                    .map_err(|_| fail("Task staging stopped"))?
            })
            .await
            .map_err(|_| fail("Task staging stopped"))?
    }

    pub async fn private_job_prepare(
        self: Arc<Self>,
        task_id: String,
        token: String,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _gate = self.fleet.gate.lock().await;
                let node = self.clone();
                let (store, target, _) = RUNTIME
                    .spawn_blocking(move || node.private_job_context())
                    .await
                    .map_err(|_| fail("Cannot open private project"))??;
                store
                    .prepare_private_code_task(
                        id(&task_id)?,
                        target,
                        &hive_core::sandbox::default_data_dir(),
                        &token,
                    )
                    .await
                    .map_err(HiveError::from)?;
                Ok(())
            })
            .await
            .map_err(|_| fail("Checkout preparation stopped"))?
    }

    pub async fn private_job_retry(self: Arc<Self>, task_id: String) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _gate = self.fleet.gate.try_lock().map_err(|_| {
                    fail("Another private operation is active. Stop it or wait for it to finish.")
                })?;
                let node = self.clone();
                let (store, target, _) = RUNTIME
                    .spawn_blocking(move || node.private_job_context())
                    .await
                    .map_err(|_| fail("Cannot open private project"))??;
                store
                    .retry_private_code_task(
                        id(&task_id)?,
                        target,
                        &hive_core::sandbox::default_data_dir(),
                    )
                    .await
                    .map_err(HiveError::from)
            })
            .await
            .map_err(|_| fail("Task recovery stopped"))?
    }

    pub async fn private_job_stop(&self) {
        if let Some(stop) = self.fleet.private_stop.lock().await.as_ref() {
            let _ = stop.send(true);
        }
    }

    pub async fn private_job_run(
        self: Arc<Self>,
        project_id: String,
        task_id: String,
    ) -> Result<String, HiveError> {
        RUNTIME.spawn(async move {
            let _gate = self.fleet.gate.try_lock().map_err(|_| fail("Another private operation is active. Wait for it to finish."))?;
            let (stop, rx) = tokio::sync::watch::channel(false);
            *self.fleet.private_stop.lock().await = Some(stop);
            let result = async {
                let node = self.clone();
                let (store, target, key) = RUNTIME.spawn_blocking(move || node.private_job_context()).await.map_err(|_| fail("Cannot open private project"))??;
                let task = id(&task_id)?;
                let statuses = store.private_code_task_statuses(id(&project_id)?, target).map_err(HiveError::from)?;
                if !statuses.iter().any(|s| s.id == task && s.status == "ready" && s.workspace.is_some()) { return Err(fail("This task is not prepared and ready on this computer")); }
                let cfg = hive_core::nodeconfig::load().map_err(HiveError::from)?;
                let (caps, ok) = crate::capabilities(&cfg).await;
                if !ok || caps.models.is_empty() { return Err(fail("Start your local model server and load a model before running this task")); }
                if caps.tools_level != hive_core::capability::ToolsLevel::SandboxedTools { return Err(fail("Enable coding tools in Settings before running this task")); }
                if *rx.borrow() { return Ok("Stopped before starting".into()); }
                let selected = statuses.iter().find(|s| s.id == task).and_then(|s| s.model_id.clone())
                    .or_else(crate::model_pref).or_else(|| caps.models.first().map(|m| m.id.clone()))
                    .ok_or_else(|| fail("Choose a coding model before running"))?;
                if !caps.models.iter().any(|m| m.id == selected) { return Err(fail("The task's model is not installed on this execution host")); }
                let probe = hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                if probe.model_tool_support(&selected).await.map_err(|_| fail("Cannot verify model tool support. Refresh models before running."))? == Some(false) {
                    return Err(fail(&format!("Model {selected} does not support coding tools. Create a task with a tool-capable model.")));
                }
                let hub = store.connect(&key).map_err(HiveError::from)?.restricted_to_card(task);
                let backend = hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                hub.check_in(&caps, None).await.map_err(HiveError::from)?;
                let worker = hive_core::worker::Worker { hub:&hub, backend:&backend, caps:&caps,
                    default_model:Some(selected), stop:rx, events:None,
                    data_dir:hive_core::sandbox::default_data_dir(), sandbox:None };
                let worked = worker.tick_with_heartbeat().await;
                let checkout = hub.check_out().await;
                let worked = worked.map_err(|e| fail(&e.to_string()))?;
                checkout.map_err(HiveError::from)?;
                Ok(if worked { "Run finished. Review the task status and output." } else { "Task remains queued. The execution slot may be busy, or the selected model may not be eligible." }.into())
            }.await;
            *self.fleet.private_stop.lock().await = None;
            result
        }).await.map_err(|_| fail("Private task execution stopped"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_preserve_arguments_and_require_zero_exit_at_workspace_root() {
        let checks = acceptance_checks(vec![PrivateTaskCheck {
            name: "Test".into(),
            command: "npm".into(),
            args: vec!["test".into(), "a b".into()],
        }])
        .unwrap();
        assert_eq!(checks[0].args, ["test", "a b"]);
        assert!(checks[0].required);
        assert_eq!(checks[0].expect_exit, 0);
        assert!(checks[0].cwd.is_none());
        assert!(acceptance_checks(vec![PrivateTaskCheck {
            name: "Test".into(),
            command: " ".into(),
            args: vec![]
        }])
        .is_err());
        assert!(acceptance_checks(vec![]).unwrap().is_empty());
    }
}
