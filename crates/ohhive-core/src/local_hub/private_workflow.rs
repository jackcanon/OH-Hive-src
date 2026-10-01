//! Persisted owner intent advances a bounded coder/checker chain through existing protocols.
use super::private_code_tasks::{
    agent_snapshot, stage_task, verified_owner, verify_target, PrivateCodeTaskRequest, RECEIPT,
    WAITING,
};
use super::*;
use crate::coder::checker::{ReviewReceipt, ReviewRequest, Verdict};

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewWorkflowRequest {
    pub request_id: Uuid,
    pub project_id: Uuid,
    pub source_task_id: Uuid,
    pub checker_node_id: Uuid,
    pub checker_agent_id: Uuid,
    pub checker_model_id: String,
    pub checker_think: Option<bool>,
    pub max_corrections: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReviewWorkflow {
    pub request: ReviewWorkflowRequest,
    pub owner: String,
    pub state: String,
    pub reason: Option<String>,
    pub current_task: Uuid,
    pub coder_task: Uuid,
    pub checking: bool,
    pub corrections: u32,
    pub deadline: i64,
    pub preparation: Uuid,
    pub run: Uuid,
    pub checker_persona: Value,
}
fn read_card(
    tx: &Transaction<'_>,
    id: Uuid,
) -> Result<(ClaimedCard, String, Option<String>, Option<String>)> {
    let (raw,status,reason,output): (String,String,Option<String>,Option<String>)=tx.query_row("SELECT c.data,c.status,c.reason,o.content FROM cards c LEFT JOIN card_outputs o ON o.card_id=c.id WHERE c.id=?1",[id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(db_error)?;
    Ok((decode(&raw)?, status, reason, output))
}
fn submission(card: &ClaimedCard) -> Result<PrivateCodeTaskRequest> {
    serde_json::from_value(card.required_capabilities[RECEIPT].clone())
        .map_err(|_| rejected("invalid workflow coding task"))
}
fn save(tx: &Transaction<'_>, flow: &ReviewWorkflow) -> Result<()> {
    tx.execute(
        "UPDATE private_review_workflows SET data=?2 WHERE id=?1",
        params![flow.request.request_id.to_string(), encode(flow)?],
    )
    .map_err(db_error)?;
    Ok(())
}
fn load(tx: &Transaction<'_>, id: Uuid, owner: &str) -> Result<ReviewWorkflow> {
    let raw: String = tx
        .query_row(
            "SELECT data FROM private_review_workflows WHERE id=?1 AND owner=?2",
            params![id.to_string(), owner],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    decode(&raw)
}
fn stop(
    tx: &Transaction<'_>,
    flow: &mut ReviewWorkflow,
    state: &str,
    reason: String,
) -> Result<()> {
    flow.state = state.into();
    flow.reason = Some(reason);
    tx.execute("INSERT OR IGNORE INTO private_run_stops SELECT r.id,r.target_node_id,?2 FROM private_runs r WHERE r.card_id=?1 AND NOT EXISTS(SELECT 1 FROM private_run_retries x WHERE x.previous_id=r.id)",params![flow.current_task.to_string(),now()]).map_err(db_error)?;
    tx.execute("UPDATE cards SET status='blocked',reason='review_workflow_stopped' WHERE id=?1 AND status IN ('ready','blocked')",[flow.current_task.to_string()]).map_err(db_error)?;
    save(tx, flow)
}
impl LocalHub {
    /// Explicit owner authorization for the entire bounded chain; no permissions come from a model.
    pub fn private_review_workflow_start(
        &self,
        request: &ReviewWorkflowRequest,
    ) -> Result<ReviewWorkflow> {
        self.with_node(|tx,node| {
            let owner=verified_owner(tx,node)?;
            if request.request_id.is_nil() || request.max_corrections>3 || request.checker_model_id.trim().is_empty() { return Err(rejected("invalid workflow bounds or checker model")); }
            check_text(&request.checker_model_id,500)?;
            let existing: Option<String>=tx.query_row("SELECT data FROM private_review_workflows WHERE id=?1",[request.request_id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
            if let Some(raw)=existing { let flow:ReviewWorkflow=decode(&raw)?; if flow.owner!=owner || flow.request!=*request {return Err(rejected("workflow request conflicts with existing work"));} return Ok(flow); }
            let (card,state,_,_)=read_card(tx,request.source_task_id)?;
            let original=submission(&card)?;
            if card.project_id!=request.project_id || original.agent_id.is_none() || original.agent_id==Some(request.checker_agent_id) || original.review_source_task_id.is_some() || !original.acceptance.iter().any(|c|c.required) || !["blocked","review","done"].contains(&state.as_str()) { return Err(rejected("choose a named coding task with required checks and a different checker")); }
            verify_target(tx,&owner,original.target_node_id)?;
            verify_target(tx,&owner,request.checker_node_id)?;
            super::private_code_tasks::validate_agent_assignment(tx,&card)?;
            let n:i64=tx.query_row("SELECT count(*) FROM private_review_workflows WHERE owner=?1 AND json_extract(data,'$.state')='active'",[&owner],|r|r.get(0)).map_err(db_error)?;
            if n>=16 {return Err(rejected("finish or stop an active workflow first"));}
            let checker=checker_submission(request,Uuid::new_v4(),card.id);
            let persona=agent_snapshot(tx,&checker)?.ok_or_else(||rejected("select an active checker"))?;
            let flow=ReviewWorkflow {request:request.clone(),owner,state:"active".into(),reason:None,current_task:card.id,coder_task:card.id,checking:false,corrections:0,deadline:now()+1800,preparation:Uuid::new_v4(),run:Uuid::new_v4(),checker_persona:persona};
            tx.execute("INSERT INTO private_review_workflows VALUES(?1,?2,?3,?4,?5)",params![request.request_id.to_string(),flow.owner,request.project_id.to_string(),request.source_task_id.to_string(),encode(&flow)?]).map_err(db_error)?;
            advance_owner(tx,&flow.owner)?;
            load(tx,request.request_id,&flow.owner)
        })
    }
    pub fn private_review_workflows(&self, project: Uuid) -> Result<Vec<ReviewWorkflow>> {
        self.with_node(|tx,node| {
            let owner=verified_owner(tx,node)?;advance_owner(tx,&owner)?;
            let mut q=tx.prepare("SELECT data FROM private_review_workflows WHERE owner=?1 AND project=?2 ORDER BY rowid DESC").map_err(db_error)?;
            let rows=q.query_map(params![owner,project.to_string()],|r|r.get::<_,String>(0)).map_err(db_error)?.map(|r|decode(&r.map_err(db_error)?)).collect(); rows
        })
    }
    pub fn private_review_workflow_stop(&self, id: Uuid) -> Result<ReviewWorkflow> {
        self.with_node(|tx, node| {
            let owner = verified_owner(tx, node)?;
            let mut flow = load(tx, id, &owner)?;
            if flow.state == "active" {
                stop(tx, &mut flow, "stopped", "Stopped by owner".into())?;
            }
            Ok(flow)
        })
    }
}
fn checker_submission(
    request: &ReviewWorkflowRequest,
    id: Uuid,
    source: Uuid,
) -> PrivateCodeTaskRequest {
    PrivateCodeTaskRequest {
        request_id: id,
        project_id: request.project_id,
        target_node_id: request.checker_node_id,
        agent_id: Some(request.checker_agent_id),
        title: "Workflow independent check".into(),
        task: "Review the frozen source against its original request and required checks.".into(),
        model_id: Some(request.checker_model_id.clone()),
        max_turns: 1,
        coding_think: request.checker_think,
        max_acceptance_repairs: 0,
        acceptance: vec![],
        review_source_task_id: Some(source),
    }
}
/// Called by authenticated owner/worker polling. Serialized transactions prevent duplicate dispatch.
pub(super) fn advance_owner(tx: &Transaction<'_>, owner: &str) -> Result<()> {
    let rows: Vec<String> = {
        let mut q=tx.prepare("SELECT data FROM private_review_workflows WHERE owner=?1 AND json_extract(data,'$.state')='active' LIMIT 16").map_err(db_error)?;
        let rows = q
            .query_map([owner], |r| r.get(0))
            .map_err(db_error)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_error)?;
        rows
    };
    for raw in rows {
        let mut flow: ReviewWorkflow = decode(&raw)?;
        tx.execute_batch("SAVEPOINT workflow_advance")
            .map_err(db_error)?;
        match advance(tx, &mut flow) {
            Ok(()) => tx
                .execute_batch("RELEASE workflow_advance")
                .map_err(db_error)?,
            Err(error) => {
                tx.execute_batch("ROLLBACK TO workflow_advance; RELEASE workflow_advance")
                    .map_err(db_error)?;
                let mut original: ReviewWorkflow = decode(&raw)?;
                stop(
                    tx,
                    &mut original,
                    "blocked",
                    format!("Workflow stopped: {error}"),
                )?;
            }
        }
    }
    Ok(())
}
fn advance(tx: &Transaction<'_>, flow: &mut ReviewWorkflow) -> Result<()> {
    if now() >= flow.deadline {
        return stop(
            tx,
            flow,
            "stopped",
            "Thirty-minute workflow deadline reached".into(),
        );
    }
    let (card, state, reason, output) = read_card(tx, flow.current_task)?;
    verify_target(tx, &flow.owner, submission(&card)?.target_node_id)?;
    super::private_code_tasks::validate_agent_assignment(tx, &card)?;
    if ["review", "done"].contains(&state.as_str()) {
        if !flow.checking {
            let id = Uuid::new_v4();
            let request = checker_submission(&flow.request, id, card.id);
            verify_target(tx, &flow.owner, request.target_node_id)?;
            let mut checker = stage_task(tx, &request)?;
            checker.required_capabilities["__hive_private_agent_v1"] = flow.checker_persona.clone();
            checker.required_capabilities["task"]=json!(format!("Owner-selected checker context: {}. This biography grants no additional tools.\n\n{}",flow.checker_persona,request.task));
            tx.execute(
                "UPDATE cards SET data=?2 WHERE id=?1",
                params![id.to_string(), encode(&checker)?],
            )
            .map_err(db_error)?;
            flow.current_task = id;
            flow.checking = true;
        } else {
            let review: ReviewRequest =
                serde_json::from_value(card.required_capabilities["independent_review"].clone())
                    .map_err(|_| rejected("invalid workflow checker"))?;
            let receipt = ReviewReceipt::from_output(
                &output.ok_or_else(|| rejected("missing workflow checker result"))?,
            )
            .map_err(|e| rejected(&e.to_string()))?;
            receipt
                .validate(card.id, &review)
                .map_err(|e| rejected(&e.to_string()))?;
            match receipt.review.verdict {
                Verdict::Pass => {
                    flow.state = "passed".into();
                    flow.reason = Some(
                        "Saved code passed independent review; no merge or publication performed"
                            .into(),
                    );
                    return save(tx, flow);
                }
                Verdict::Inconclusive => {
                    return stop(
                        tx,
                        flow,
                        "blocked",
                        "Independent review inconclusive; owner review required".into(),
                    )
                }
                Verdict::ChangesRequired => {
                    if flow.corrections >= flow.request.max_corrections {
                        return stop(
                            tx,
                            flow,
                            "blocked",
                            "Correction limit reached; owner review required".into(),
                        );
                    }
                    let request = super::private_correction::PrivateCorrectionRequest {
                        request_id: Uuid::new_v4(),
                        project_id: flow.request.project_id,
                        review_task_id: card.id,
                    };
                    let correction = super::private_correction::stage(tx, &request, &flow.owner)?;
                    flow.current_task = correction.id;
                    flow.coder_task = correction.id;
                    flow.checking = false;
                    flow.corrections += 1;
                }
            }
        }
        flow.preparation = Uuid::new_v4();
        flow.run = Uuid::new_v4();
        return save(tx, flow);
    }
    if state == "blocked" && reason.as_deref() == Some(WAITING) {
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT id,state FROM private_preparations WHERE card_id=?1",
                [card.id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_error)?;
        if let Some((id, _)) = existing {
            flow.preparation =
                Uuid::parse_str(&id).map_err(|_| rejected("invalid preparation identity"))?;
        } else {
            super::private_preparation::request(tx, &flow.owner, flow.preparation, card.id)?;
        }
    } else if state == "blocked" && reason.as_deref() == Some("awaiting_private_run") {
        super::private_run::request(tx, &flow.owner, flow.run, card.id)?;
    } else {
        let run: Option<String>=tx.query_row("SELECT id FROM private_runs WHERE card_id=?1 AND NOT EXISTS(SELECT 1 FROM private_run_retries x WHERE x.previous_id=private_runs.id)",[card.id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
        let Some(run) = run else {
            return Err(rejected(
                "task is not awaiting workflow preparation or execution",
            ));
        };
        let status = super::private_run::status(
            tx,
            Uuid::parse_str(&run).map_err(|_| rejected("invalid run"))?,
        )?;
        if !["queued", "running"].contains(&status.state.as_str()) || status.stop_requested {
            return stop(
                tx,
                flow,
                "blocked",
                format!("Execution {} requires owner review", status.state),
            );
        }
    }
    save(tx, flow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::{AgentRuntimeKind, NewAgentProfile};
    use crate::coder::checker::{
        CaptureIdentity, Finding, ReviewFile, ReviewPackage, ReviewSnapshot, ReviewVerdict,
    };
    use crate::coder::{AcceptanceOutcome, AcceptanceResult};
    use sha2::{Digest, Sha256};
    fn fixture() -> (LocalHubStore, LocalHub, ReviewWorkflowRequest) {
        let store = LocalHubStore::in_memory().unwrap();
        let node = store.enroll_owner("worker").unwrap();
        let owner = Uuid::new_v4();
        store.set_node_owner(node.node_id, owner).unwrap();
        store.transaction(|tx|{tx.execute("UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust='fixture' WHERE id=1",params![Uuid::new_v4().to_string(),owner.to_string()]).unwrap();tx.execute("INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",params![Uuid::new_v4().to_string(),node.node_id.to_string(),now()]).unwrap();Ok(())}).unwrap();
        let hub = store.connect(&node.raw_key).unwrap();
        let project = store.create_project("workflow", "fixture").unwrap();
        store
            .set_project_repository(
                project,
                Some(&repository::ProjectRepository {
                    repo_url: "https://github.com/example/fixture.git".into(),
                    repo_ref: None,
                }),
            )
            .unwrap();
        let agent = |name: &str| {
            store
                .bots_agents_create(NewAgentProfile {
                    owner,
                    name: name.into(),
                    runtime_kind: AgentRuntimeKind::Local,
                    preferred_host: Some(node.node_id),
                    capability_policy_ref: "developer-v1".into(),
                    provider_account_ref: None,
                    memory_namespace: "fixture".into(),
                })
                .unwrap()
        };
        let coder = agent("coder");
        let checker = agent("checker");
        let card=hub.private_code_task_stage(&PrivateCodeTaskRequest {request_id:Uuid::new_v4(),project_id:project,target_node_id:node.node_id,agent_id:Some(coder.id),title:"sum".into(),task:"Add two numbers".into(),model_id:Some("fixture".into()),max_turns:8,coding_think:Some(false),max_acceptance_repairs:0,acceptance:serde_json::from_value(json!([{"name":"tests","command":"python3","args":["-m","unittest"],"expect_exit":0,"required":true}])).unwrap(),review_source_task_id:None}).unwrap();
        (
            store,
            hub,
            ReviewWorkflowRequest {
                request_id: Uuid::new_v4(),
                project_id: project,
                source_task_id: card.id,
                checker_node_id: node.node_id,
                checker_agent_id: checker.id,
                checker_model_id: "fixture".into(),
                checker_think: Some(false),
                max_corrections: 1,
            },
        )
    }
    fn finish(store: &LocalHubStore, task: Uuid, output: &str) {
        store.transaction(|tx|{tx.execute("UPDATE cards SET status='review',reason=NULL WHERE id=?1",[task.to_string()]).unwrap();tx.execute("INSERT OR REPLACE INTO card_outputs(card_id,node_id,session,content,usage) VALUES(?1,'fixture','fixture',?2,'{}')",params![task.to_string(),output]).unwrap();Ok(())}).unwrap();
    }
    fn coder_output(store: &LocalHubStore, task: Uuid) -> String {
        store
            .transaction(|tx| {
                let (card, _, _, _) = read_card(tx, task)?;
                let request = submission(&card)?;
                let snapshot = ReviewSnapshot {
                    identity: CaptureIdentity {
                        task_id: task,
                        agent_id: request.agent_id.unwrap(),
                        original_task: request.task,
                    },
                    base_commit: "a".repeat(40),
                    files: vec![ReviewFile {
                        path: "add.py".into(),
                        before: None,
                        after: Some("return a - b\n".into()),
                    }],
                    coder_report: "Done".into(),
                    acceptance: AcceptanceOutcome::Passed(vec![AcceptanceResult {
                        name: "weak test".into(),
                        command_line: "test".into(),
                        exit_status: Some(0),
                        passed: true,
                        required: true,
                        timed_out: false,
                        stdout_tail: String::new(),
                        stderr_tail: String::new(),
                        error: None,
                    }]),
                    model_id: Some("fixture".into()),
                };
                let package = ReviewPackage {
                    digest: format!(
                        "{:x}",
                        Sha256::digest(serde_json::to_vec(&snapshot).unwrap())
                    ),
                    snapshot,
                };
                Ok(format!(
                    "Done\nIndependent review package: {}",
                    encode(&package)?
                ))
            })
            .unwrap()
    }
    fn checker_output(store: &LocalHubStore, task: Uuid, verdict: Verdict) -> String {
        store
            .transaction(|tx| {
                let (card, _, _, _) = read_card(tx, task)?;
                let r: ReviewRequest = serde_json::from_value(
                    card.required_capabilities["independent_review"].clone(),
                )
                .unwrap();
                let receipt = ReviewReceipt {
                    checker_task_id: task,
                    checker_agent_id: r.checker_agent_id,
                    source_task_id: r.package.snapshot.identity.task_id,
                    source_agent_id: r.package.snapshot.identity.agent_id,
                    package_digest: r.package.digest,
                    base_commit: r.package.snapshot.base_commit,
                    model_id: Some("fixture".into()),
                    review: ReviewVerdict {
                        summary: "Fixture review".into(),
                        findings: if verdict == Verdict::ChangesRequired {
                            vec![Finding {
                                path: "add.py".into(),
                                line: 1,
                                message: "Subtracts".into(),
                                evidence: "return a - b".into(),
                            }]
                        } else {
                            vec![]
                        },
                        verdict,
                    },
                    scope: "frozen_snapshot_only".into(),
                    test_execution: "prior_coder_host_receipt_only".into(),
                };
                Ok(format!(
                    "Independent checker verdict: {}",
                    encode(&receipt)?
                ))
            })
            .unwrap()
    }
    #[tokio::test]
    async fn workflow_transport_rejects_bad_credentials_and_replays_owner_intent() {
        let (store, hub, request) = fixture();
        let flow = hub.private_review_workflow_start(&request).unwrap();
        let node = store.enroll_owner("remote controller").unwrap();
        store
            .set_node_owner(node.node_id, Uuid::parse_str(&flow.owner).unwrap())
            .unwrap();
        store
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",
                    params![Uuid::new_v4().to_string(), node.node_id.to_string(), now()],
                )
                .map_err(db_error)?;
                Ok(())
            })
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            transport::serve(store, listener, async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        let remote = transport::RemoteLocalHub::new(&url, node.raw_key).unwrap();
        assert_eq!(
            remote
                .private_review_workflow_start(&request)
                .await
                .unwrap()
                .preparation,
            flow.preparation
        );
        assert_eq!(
            remote
                .private_review_workflows(request.project_id)
                .await
                .unwrap()
                .len(),
            1
        );
        let invalid = transport::RemoteLocalHub::new(&url, "bad-key".into()).unwrap();
        assert!(invalid
            .private_review_workflow_start(&request)
            .await
            .is_err());
        assert!(invalid
            .private_review_workflows(request.project_id)
            .await
            .is_err());
        assert!(invalid
            .private_review_workflow_stop(request.request_id)
            .await
            .is_err());
        assert_eq!(
            remote
                .private_review_workflow_stop(request.request_id)
                .await
                .unwrap()
                .state,
            "stopped"
        );
        stop.send(()).unwrap();
        server.await.unwrap();
    }
    #[test]
    fn workflow_dispatch_is_durable_idempotent_and_stop_cancels_preparation() {
        let (store, hub, request) = fixture();
        let flow = hub.private_review_workflow_start(&request).unwrap();
        assert_eq!(flow.state, "active");
        assert!(hub.private_coding_pending().unwrap().preparation);
        assert_eq!(
            hub.private_review_workflow_start(&request)
                .unwrap()
                .preparation,
            flow.preparation
        );
        let work = hub.private_preparation_take().unwrap().unwrap();
        assert_eq!(work.card.id, request.source_task_id);
        hub.private_preparation_complete(work.operation_id, "/tmp/fixture-checkout")
            .unwrap();
        let pending = hub.private_coding_pending().unwrap();
        assert_eq!(pending.run, Some(flow.run));
        finish(
            &store,
            request.source_task_id,
            &coder_output(&store, request.source_task_id),
        );
        let next = hub
            .private_review_workflows(request.project_id)
            .unwrap()
            .remove(0);
        assert!(next.checking);
        assert_ne!(next.current_task, request.source_task_id);
        let node = store.enroll_owner("same owner new controller").unwrap();
        store
            .set_node_owner(node.node_id, Uuid::parse_str(&next.owner).unwrap())
            .unwrap();
        store
            .transaction(|tx| {
                tx.execute(
                    "INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",
                    params![Uuid::new_v4().to_string(), node.node_id.to_string(), now()],
                )
                .map_err(db_error)?;
                Ok(())
            })
            .unwrap();
        let restarted = store.connect(&node.raw_key).unwrap();
        assert_eq!(
            restarted
                .private_review_workflows(request.project_id)
                .unwrap()[0]
                .current_task,
            next.current_task
        );
        let checker_work = hub.private_preparation_take().unwrap().unwrap();
        assert_eq!(checker_work.card.id, next.current_task);
        hub.private_preparation_complete(checker_work.operation_id, "/tmp/fixture-checker")
            .unwrap();
        hub.private_coding_pending().unwrap();
        finish(
            &store,
            next.current_task,
            &checker_output(&store, next.current_task, Verdict::ChangesRequired),
        );
        let corrected = hub
            .private_review_workflows(request.project_id)
            .unwrap()
            .remove(0);
        assert_eq!(corrected.corrections, 1);
        assert!(!corrected.checking);
        hub.private_coding_pending().unwrap();
        hub.private_review_workflow_stop(request.request_id)
            .unwrap();
        assert!(!hub.private_coding_pending().unwrap().preparation);
        assert!(hub.private_preparation_take().unwrap().is_none());
        let count: i64 = store
            .transaction(|tx| {
                Ok(tx
                    .query_row("SELECT count(*) FROM cards", [], |r| r.get(0))
                    .unwrap())
            })
            .unwrap();
        assert_eq!(count, 3); // Repeated polls never duplicate cards.
    }
    #[test]
    fn workflow_pass_uncertainty_and_zero_correction_limit_stop_without_extra_tasks() {
        for verdict in [
            Verdict::Pass,
            Verdict::Inconclusive,
            Verdict::ChangesRequired,
        ] {
            let (store, hub, mut request) = fixture();
            request.max_corrections = 0;
            finish(
                &store,
                request.source_task_id,
                &coder_output(&store, request.source_task_id),
            );
            let checker = hub.private_review_workflow_start(&request).unwrap();
            assert!(checker.checking);
            finish(
                &store,
                checker.current_task,
                &checker_output(&store, checker.current_task, verdict.clone()),
            );
            let flow = hub
                .private_review_workflows(request.project_id)
                .unwrap()
                .remove(0);
            assert_eq!(
                flow.state,
                if verdict == Verdict::Pass {
                    "passed"
                } else {
                    "blocked"
                }
            );
            assert_eq!(flow.corrections, 0);
            let current = flow.current_task;
            assert_eq!(
                hub.private_review_workflows(request.project_id).unwrap()[0].current_task,
                current
            );
        }
    }
    #[test]
    fn workflow_stop_and_deadline_fence_queued_runs_and_do_not_replay_failure() {
        for expired in [false, true] {
            let (store, hub, request) = fixture();
            let flow = hub.private_review_workflow_start(&request).unwrap();
            let work = hub.private_preparation_take().unwrap().unwrap();
            hub.private_preparation_complete(work.operation_id, "/tmp/fixture")
                .unwrap();
            assert_eq!(hub.private_coding_pending().unwrap().run, Some(flow.run));
            if expired {
                store
                    .transaction(|tx| {
                        let mut e = load(tx, request.request_id, &flow.owner)?;
                        e.deadline = now() - 1;
                        save(tx, &e)
                    })
                    .unwrap();
            } else {
                hub.private_review_workflow_stop(request.request_id)
                    .unwrap();
            }
            assert!(hub.private_run_status(flow.run).unwrap().stop_requested);
            assert!(hub.private_coding_pending().unwrap().run.is_none());
            assert_eq!(
                hub.private_review_workflows(request.project_id).unwrap()[0].state,
                "stopped"
            );
        }
        let (store, hub, request) = fixture();
        let flow = hub.private_review_workflow_start(&request).unwrap();
        let work = hub.private_preparation_take().unwrap().unwrap();
        hub.private_preparation_complete(work.operation_id, "/tmp/fixture")
            .unwrap();
        hub.private_coding_pending().unwrap();
        store
            .transaction(|tx| {
                tx.execute(
                    "UPDATE cards SET status='blocked',reason='execution_failed' WHERE id=?1",
                    [request.source_task_id.to_string()],
                )
                .map_err(db_error)?;
                tx.execute(
                    "UPDATE private_runs SET state='running' WHERE id=?1",
                    [flow.run.to_string()],
                )
                .map_err(db_error)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            hub.private_review_workflows(request.project_id).unwrap()[0].state,
            "blocked"
        );
        assert!(hub.private_coding_pending().unwrap().run.is_none());
    }
    #[test]
    fn workflow_bounds_authorization_tampering_and_deadline_fail_closed() {
        let (store, hub, mut request) = fixture();
        request.max_corrections = 4;
        assert!(hub.private_review_workflow_start(&request).is_err());
        request.max_corrections = 1;
        let flow = hub.private_review_workflow_start(&request).unwrap();
        let mut conflicting = request.clone();
        conflicting.checker_model_id = "other".into();
        assert!(hub.private_review_workflow_start(&conflicting).is_err());
        let stranger = store.enroll_owner("stranger").unwrap();
        store
            .set_node_owner(stranger.node_id, Uuid::new_v4())
            .unwrap();
        assert!(store
            .connect(&stranger.raw_key)
            .unwrap()
            .private_review_workflow_stop(request.request_id)
            .is_err());
        store
            .transaction(|tx| {
                let mut expired = flow.clone();
                expired.deadline = now() - 1;
                save(tx, &expired)
            })
            .unwrap();
        assert_eq!(
            hub.private_review_workflows(request.project_id).unwrap()[0].state,
            "stopped"
        );
        assert!(!hub.private_coding_pending().unwrap().preparation);
        let (store, hub, request) = fixture();
        finish(
            &store,
            request.source_task_id,
            &coder_output(&store, request.source_task_id),
        );
        let flow = hub.private_review_workflow_start(&request).unwrap();
        finish(&store, flow.current_task, "Invalid verdict");
        assert_eq!(
            hub.private_review_workflows(request.project_id).unwrap()[0].state,
            "blocked"
        );
    }
}
