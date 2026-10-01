//! Native remote coding controller and explicitly opened secondary worker.
use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::{
    local_hub::{private_code_tasks::PrivateCodeTaskRequest, RemoteLocalHub},
    nodeconfig,
};
use std::sync::Arc;
use tokio::sync::{watch, Mutex};
use uuid::Uuid;
fn fail(s: &str) -> HiveError {
    HiveError::Failed(s.into())
}
fn id(s: &str) -> Result<Uuid, HiveError> {
    Uuid::parse_str(s).map_err(|_| fail("Invalid operation identity"))
}

#[derive(Clone, uniffi::Record)]
pub struct RemoteCodingTask {
    pub id: String,
    pub target: String,
    pub title: String,
    pub agent_name: Option<String>,
    pub agent_id: Option<String>,
    pub review_available: bool,
    pub review_source_task_id: Option<String>,
    pub checker_verdict: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub output: Option<String>,
    pub check_count: u32,
    pub preparation_id: Option<String>,
    pub preparation_state: Option<String>,
    pub run_id: Option<String>,
    pub run_state: Option<String>,
    pub lease_active: bool,
}
#[derive(Clone, uniffi::Record)]
pub struct CodingReviewWorkflow {
    pub id: String,
    pub source: String,
    pub current: String,
    pub state: String,
    pub reason: Option<String>,
    pub corrections: u32,
}
impl From<hive_core::local_hub::private_workflow::ReviewWorkflow> for CodingReviewWorkflow {
    fn from(w: hive_core::local_hub::private_workflow::ReviewWorkflow) -> Self {
        Self {
            id: w.request.request_id.to_string(),
            source: w.request.source_task_id.to_string(),
            current: w.current_task.to_string(),
            state: w.state,
            reason: w.reason,
            corrections: w.corrections,
        }
    }
}
#[uniffi::export]
impl HiveNode {
    pub async fn coding_review_workflows(
        self: Arc<Self>,
        project: String,
    ) -> Result<Vec<CodingReviewWorkflow>, HiveError> {
        RUNTIME
            .spawn_blocking(move || {
                let (store, _, key) = self.private_job_context()?;
                Ok(store
                    .connect(&key)?
                    .private_review_workflows(id(&project)?)?
                    .into_iter()
                    .map(Into::into)
                    .collect())
            })
            .await
            .map_err(|_| fail("Could not load review workflows"))?
    }
    pub async fn coding_review_workflow_stop(
        self: Arc<Self>,
        workflow: String,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn_blocking(move || {
                let (store, _, key) = self.private_job_context()?;
                store
                    .connect(&key)?
                    .private_review_workflow_stop(id(&workflow)?)?;
                Ok(())
            })
            .await
            .map_err(|_| fail("Could not stop review workflow"))?
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn coding_review_workflow_start(
        self: Arc<Self>,
        request: String,
        project: String,
        source: String,
        target: String,
        agent: String,
        model: String,
        coding_think: Option<bool>,
        corrections: u32,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn_blocking(move || {
                let (store, _, key) = self.private_job_context()?;
                store.connect(&key)?.private_review_workflow_start(
                    &hive_core::local_hub::private_workflow::ReviewWorkflowRequest {
                        request_id: id(&request)?,
                        project_id: id(&project)?,
                        source_task_id: id(&source)?,
                        checker_node_id: id(&target)?,
                        checker_agent_id: id(&agent)?,
                        checker_model_id: model,
                        checker_think: coding_think,
                        max_corrections: corrections,
                    },
                )?;
                Ok(())
            })
            .await
            .map_err(|_| fail("Could not start review workflow"))?
    }
}
#[uniffi::export]
impl HiveNode {
    pub async fn remote_coding_tasks(
        self: Arc<Self>,
        project: String,
    ) -> Result<Vec<RemoteCodingTask>, HiveError> {
        RUNTIME
            .spawn_blocking(move || {
                let (store, _, key) = self.private_job_context()?;
                Ok(store
                    .connect(&key)?
                    .private_coding_tasks(id(&project)?)?
                    .into_iter()
                    .map(|t| RemoteCodingTask {
                        id: t.task_id.to_string(),
                        target: t.target_name,
                        title: t.title,
                        agent_name: t.agent_name,
                        agent_id: t.agent_id.map(|id| id.to_string()),
                        review_available: t.review_available,
                        review_source_task_id: t.review_source_task_id.map(|id| id.to_string()),
                        checker_verdict: t.checker_verdict,
                        status: t.status,
                        reason: t.reason,
                        output: t.output,
                        check_count: t.check_count,
                        preparation_id: t.preparation_id.map(|id| id.to_string()),
                        preparation_state: t.preparation_state,
                        run_id: t.run.as_ref().map(|r| r.operation_id.to_string()),
                        run_state: t.run.as_ref().map(|r| r.state.clone()),
                        lease_active: t.run.is_some_and(|r| r.lease_active),
                    })
                    .collect())
            })
            .await
            .map_err(|_| fail("Could not load coding tasks"))?
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn remote_coding_stage(
        self: Arc<Self>,
        request: String,
        project: String,
        target: String,
        title: String,
        task: String,
        model: String,
        turns: u32,
        checks: Vec<crate::private_jobs::PrivateTaskCheck>,
    ) -> Result<(), HiveError> {
        self.remote_coding_stage_for_agent(
            request, project, target, title, task, model, turns, checks, None,
        )
        .await
    }
    /// Only owner-selected local agents; selecting a template alone grants no execution.
    pub async fn private_coding_agents(
        self: Arc<Self>,
    ) -> Result<Vec<crate::bots::BotsAgent>, HiveError> {
        RUNTIME
            .spawn_blocking(move || {
                let (store, _, key) = self.private_job_context()?;
                let hub = store.connect(&key)?;
                hub.private_execution_hosts()?; // verified fleet before reading its agent list
                Ok(hub
                    .bots_agents_list()?
                    .into_iter()
                    .filter(|a| {
                        !a.archived
                            && a.runtime_kind == hive_core::bots::AgentRuntimeKind::Local
                            && a.preferred_host.is_some()
                    })
                    .map(crate::bots::BotsAgent::from)
                    .collect())
            })
            .await
            .map_err(|_| fail("Could not load project agents"))?
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn remote_coding_stage_for_agent(
        self: Arc<Self>,
        request: String,
        project: String,
        target: String,
        title: String,
        task: String,
        model: String,
        turns: u32,
        checks: Vec<crate::private_jobs::PrivateTaskCheck>,
        agent: Option<String>,
    ) -> Result<(), HiveError> {
        self.remote_coding_stage_with_reasoning(
            request, project, target, title, task, model, turns, checks, agent, None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn remote_coding_stage_with_reasoning(
        self: Arc<Self>,
        request: String,
        project: String,
        target: String,
        title: String,
        task: String,
        model: String,
        turns: u32,
        checks: Vec<crate::private_jobs::PrivateTaskCheck>,
        agent: Option<String>,
        coding_think: Option<bool>,
    ) -> Result<(), HiveError> {
        self.stage_coding_request(
            request,
            project,
            target,
            title,
            task,
            model,
            turns,
            checks,
            agent,
            coding_think,
            None,
        )
        .await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn remote_coding_stage_review(
        self: Arc<Self>,
        request: String,
        project: String,
        target: String,
        source: String,
        agent: String,
        model: String,
        coding_think: Option<bool>,
    ) -> Result<(), HiveError> {
        self.stage_coding_request(
            request,
            project,
            target,
            "Independent code check".into(),
            "Review the source task's frozen files against its original request.".into(),
            model,
            1,
            vec![],
            Some(agent),
            coding_think,
            Some(source),
        )
        .await
    }
    pub async fn remote_coding_stage_correction(
        self: Arc<Self>,
        request: String,
        project: String,
        review: String,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn_blocking(move || {
                let (store, _, key) = self.private_job_context()?;
                store.connect(&key)?.private_code_correction_stage(
                    &hive_core::local_hub::private_correction::PrivateCorrectionRequest {
                        request_id: id(&request)?,
                        project_id: id(&project)?,
                        review_task_id: id(&review)?,
                    },
                )?;
                Ok(())
            })
            .await
            .map_err(|_| fail("Could not save correction task"))?
    }
    /// Stable request IDs come from the UI. Repeat delivery cannot create an extra attempt.
    pub async fn remote_coding_command(
        self: Arc<Self>,
        action: String,
        task: String,
        operation: String,
        request: String,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _gate = self.fleet.gate.lock().await;
                let (store, _, key) = self.private_job_context()?;
                let h = store.connect(&key)?;
                match action.as_str() {
                    "prepare" => {
                        h.private_preparation_request(id(&request)?, id(&task)?)?;
                    }
                    "run" => {
                        h.private_run_request(id(&request)?, id(&task)?)?;
                    }
                    "stop" => {
                        h.private_run_stop(id(&operation)?)?;
                    }
                    "retry" => {
                        h.private_run_retry(id(&operation)?, id(&request)?)?;
                    }
                    "recover" => {
                        h.private_preparation_recover(id(&request)?, id(&operation)?)?;
                    }
                    _ => return Err(fail("Unknown coding operation")),
                }
                Ok(())
            })
            .await
            .map_err(|_| fail("Coding operation stopped"))?
    }
    /// No fallback: this opt-in worker only connects to a verified selected primary.
    pub async fn private_coding_worker_open(
        self: Arc<Self>,
    ) -> Result<Arc<PrivateCodingWorker>, HiveError> {
        RUNTIME
            .spawn(async move {
                let _gate = self.fleet.gate.lock().await;
                let (selection, wire) = crate::private_fleet::selected()?
                    .ok_or_else(|| fail("Connect this execution computer to your primary first"))?;
                let remote = selection.connect().await?.into_transport();
                let (stop, _) = watch::channel(false);
                Ok(Arc::new(PrivateCodingWorker {
                    node: self.clone(),
                    remote,
                    selection: wire,
                    stop,
                    operation: Mutex::new(()),
                    advertisement: Mutex::new(()),
                }))
            })
            .await
            .map_err(|_| fail("Could not open coding worker"))?
    }
}
#[derive(uniffi::Object)]
pub struct PrivateCodingWorker {
    node: Arc<HiveNode>,
    remote: RemoteLocalHub,
    selection: String,
    stop: watch::Sender<bool>,
    operation: Mutex<()>,
    advertisement: Mutex<()>,
}
impl PrivateCodingWorker {
    fn validate(&self) -> Result<(), HiveError> {
        if *self.stop.borrow() {
            return Err(fail("Coding worker is stopped"));
        }
        if crate::private_fleet::selected_wire()?.as_deref() != Some(&self.selection) {
            return Err(fail(
                "Primary selection changed. Restart the coding worker.",
            ));
        }
        Ok(())
    }
}
#[uniffi::export]
impl PrivateCodingWorker {
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
    pub async fn advertise(self: Arc<Self>, git_connected: bool) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _advertisement = self.advertisement.lock().await;
                // A disabled report is useful on explicit Stop; no model query is needed then.
                if *self.stop.borrow() {
                    self.remote
                        .private_coding_advertise(
                            &hive_core::local_hub::private_readiness::CodingReadiness {
                                worker_enabled: false,
                                coding_enabled: false,
                                git_connected,
                                models: vec![],
                            },
                        )
                        .await?;
                    return Ok(());
                }
                self.validate()?;
                let cfg = nodeconfig::load()?;
                let backend = hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                self.remote
                    .refresh_private_coding_readiness(
                        &backend,
                        true,
                        cfg.tools_level == hive_core::capability::ToolsLevel::SandboxedTools,
                        git_connected,
                    )
                    .await?;
                Ok(())
            })
            .await
            .map_err(|_| fail("Readiness refresh stopped"))?
    }
    pub async fn next_work(self: Arc<Self>) -> Result<String, HiveError> {
        RUNTIME
            .spawn(async move {
                self.validate()?;
                if nodeconfig::load()?.tools_level
                    != hive_core::capability::ToolsLevel::SandboxedTools
                {
                    return Err(fail(
                        "Enable coding tools on this execution computer before continuing",
                    ));
                }
                let pending = self.remote.private_coding_pending().await?;
                Ok(if pending.preparation {
                    "prepare"
                } else if pending.run.is_some() {
                    "run"
                } else {
                    "idle"
                }
                .into())
            })
            .await
            .map_err(|_| fail("Work discovery stopped"))?
    }
    pub async fn tick(self: Arc<Self>, token: String) -> Result<String, HiveError> {
        RUNTIME
            .spawn(async move {
                let _one = self
                    .operation
                    .try_lock()
                    .map_err(|_| fail("This worker is already busy"))?;
                let _selection = self.node.fleet.gate.lock().await;
                self.validate()?;
                let cfg = nodeconfig::load()?;
                if cfg.tools_level != hive_core::capability::ToolsLevel::SandboxedTools {
                    return Err(fail(
                        "Enable coding tools on this execution computer before continuing",
                    ));
                }
                let pending = self.remote.private_coding_pending().await?;
                if pending.preparation {
                    self.remote
                        .prepare_next_private_checkout(
                            &hive_core::sandbox::default_data_dir(),
                            &token,
                        )
                        .await?;
                    return Ok("Checkout prepared; waiting for Run from the primary.".into());
                }
                if let Some(run) = pending.run {
                    let backend =
                        hive_core::backend::llama_cpp::LlamaCppBackend::new(&cfg.llama_url);
                    let status = self
                        .remote
                        .execute_private_run(
                            run,
                            &backend,
                            true,
                            &hive_core::sandbox::default_data_dir(),
                            self.stop.subscribe(),
                        )
                        .await?;
                    return Ok(format!("Task {}", status.state));
                }
                Ok("Waiting for a task from your primary.".into())
            })
            .await
            .map_err(|_| fail("Coding worker stopped"))?
    }
}

impl HiveNode {
    #[allow(clippy::too_many_arguments)]
    async fn stage_coding_request(
        self: Arc<Self>,
        request: String,
        project: String,
        target: String,
        title: String,
        task: String,
        model: String,
        turns: u32,
        checks: Vec<crate::private_jobs::PrivateTaskCheck>,
        agent: Option<String>,
        coding_think: Option<bool>,
        review_source: Option<String>,
    ) -> Result<(), HiveError> {
        RUNTIME.spawn(async move{
            let _gate=self.fleet.gate.lock().await;
            let (store,_,key)=self.private_job_context()?;
            let hub=store.connect(&key)?;
            let target=id(&target)?;
            let hosts=hub.private_coding_hosts()?;
            let host=hosts.iter().find(|h|h.host.node_id==target).ok_or_else(||fail("Choose an enrolled execution computer"))?;
            let report=host.report.as_ref().filter(|r|host.fresh && r.worker_enabled && r.coding_enabled).ok_or_else(||fail("Execution computer is not ready. Enable its private coding worker and refresh."))?;
            if !report.models.iter().any(|m|m.id==model && m.supports_tools!=Some(false)){return Err(fail("Choose a model available on that computer"));}
            hub.private_code_task_stage(&PrivateCodeTaskRequest{ review_source_task_id: review_source.as_deref().map(id).transpose()?,coding_think,max_acceptance_repairs:0,request_id:id(&request)?,project_id:id(&project)?,target_node_id:target,agent_id:agent.as_deref().map(id).transpose()?,title,task,model_id:Some(model),max_turns:turns,acceptance:crate::private_jobs::acceptance_checks(checks)?})?;
            Ok(())
        }).await.map_err(|_|fail("Task submission stopped"))?
    }
}
