//! Read-only dispatch views. Requests and claims still use the durable operation protocols.
use super::private_code_tasks::{verified_owner, verify_target, PrivateCodeTaskRequest, RECEIPT};
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedCodingFile {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CodingTaskOverview {
    #[serde(default)]
    pub saved_files: Vec<SavedCodingFile>,
    pub task_id: Uuid,
    pub target_node_id: Uuid,
    pub target_name: String,
    pub title: String,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub agent_id: Option<Uuid>,
    #[serde(default)]
    pub review_available: bool,
    #[serde(default)]
    pub review_source_task_id: Option<Uuid>,
    #[serde(default)]
    pub checker_verdict: Option<String>,
    pub status: String,
    pub reason: Option<String>,
    pub output: Option<String>,
    pub check_count: u32,
    pub preparation_id: Option<Uuid>,
    pub preparation_state: Option<String>,
    pub run: Option<super::private_run::PrivateRunStatus>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CodingPendingWork {
    pub preparation: bool,
    pub run: Option<Uuid>,
}
impl LocalHub {
    pub fn private_coding_tasks(&self, project: Uuid) -> Result<Vec<CodingTaskOverview>> {
        self.with_node(|tx,node| {
            let owner = verified_owner(tx,node)?;
            let mut q = tx.prepare("SELECT c.data,c.status,c.reason,(SELECT substr(content,1,128000) FROM card_outputs WHERE card_id=c.id) FROM cards c WHERE c.project_id=?1 ORDER BY c.rowid DESC").map_err(db_error)?;
            let rows = q.query_map([project.to_string()], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?))).map_err(db_error)?;
            let mut tasks = Vec::new();
            for row in rows {
                let (raw,status,reason,output) = row.map_err(db_error)?;
                let card: ClaimedCard = decode(&raw)?;
                let Some(value) = card.required_capabilities.get(RECEIPT) else { continue; };
                let request: PrivateCodeTaskRequest = serde_json::from_value(value.clone()).map_err(|_| rejected("invalid private submission"))?;
                if verify_target(tx,&owner,request.target_node_id).is_err() { continue; }
                let target_name = tx.query_row("SELECT name FROM nodes WHERE id=?1",[request.target_node_id.to_string()],|r|r.get(0)).map_err(db_error)?;
                let prep: Option<(String,String)> = tx.query_row("SELECT id,state FROM private_preparations WHERE card_id=?1",[card.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(db_error)?;
                let run: Option<String> = tx.query_row("SELECT id FROM private_runs WHERE card_id=?1 AND NOT EXISTS(SELECT 1 FROM private_run_retries WHERE previous_id=private_runs.id)",[card.id.to_string()],|r|r.get(0)).optional().map_err(db_error)?;
                let review_available = review_available(output.as_deref()) && ["review", "done"].contains(&status.as_str());
                let checker_verdict = request.review_source_task_id.and_then(|_| output.as_deref().and_then(|o| o.lines().find_map(|l| l.strip_prefix("Independent checker verdict: "))).and_then(|s| serde_json::from_str::<Value>(s).ok()).and_then(|v| v["review"]["verdict"].as_str().map(str::to_owned)));
                let saved_files = saved_coding_files(output.as_deref());
                let output = output.map(|o| if request.review_source_task_id.is_some() { display_checker_output(&o) } else { o }).map(|o| o.lines().filter(|l| !l.starts_with("Independent review package: ")).collect::<Vec<_>>().join("\n"));
                tasks.push(CodingTaskOverview { saved_files, agent_id:request.agent_id,review_available,review_source_task_id:request.review_source_task_id,checker_verdict, task_id:card.id,target_node_id:request.target_node_id,target_name,agent_name:card.required_capabilities.get("__hive_private_agent_v1").and_then(|v|v.get("name")).and_then(Value::as_str).map(str::to_owned),title:card.title,status,reason,output,check_count:request.acceptance.len() as u32,
                    preparation_id:prep.as_ref().map(|p|Uuid::parse_str(&p.0)).transpose().map_err(|_|rejected("invalid preparation"))?,
                    preparation_state:prep.map(|p|p.1),run:run.map(|id| super::private_run::status(tx,Uuid::parse_str(&id).map_err(|_|rejected("invalid run"))?)).transpose()? });
            }
            Ok(tasks)
        })
    }
    /// Only the authenticated target's work. Does not claim or authorize execution.
    pub fn private_coding_pending(&self) -> Result<CodingPendingWork> {
        self.with_node(|tx,node| {
            let owner=verified_owner(tx,node)?;
            #[cfg(feature = "sandbox")]
            super::private_workflow::advance_owner(tx,&owner)?;
            verify_target(tx,&owner,Uuid::parse_str(node).map_err(|_|rejected("invalid node"))?)?;
            let preparation: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM private_preparations p WHERE p.target_node_id=?1 AND (p.state='queued' OR (p.state='claimed' AND p.session=?2)) AND NOT EXISTS(SELECT 1 FROM private_preparation_recoveries x WHERE x.operation_id=p.id AND x.retired_session=?2) AND NOT EXISTS(SELECT 1 FROM private_review_workflows w WHERE json_extract(w.data,'$.current_task')=p.card_id AND json_extract(w.data,'$.state')!='active'))",params![node,self.session.to_string()],|r|r.get(0)).map_err(db_error)?;
            let run:Option<String>=tx.query_row("SELECT r.id FROM private_runs r WHERE r.target_node_id=?1 AND r.state='queued' AND NOT EXISTS(SELECT 1 FROM private_run_stops s WHERE s.operation_id=r.id) AND NOT EXISTS(SELECT 1 FROM private_run_retries x WHERE x.previous_id=r.id) ORDER BY r.created,r.rowid LIMIT 1",[node],|r|r.get(0)).optional().map_err(db_error)?;
            Ok(CodingPendingWork { preparation,run:run.map(|id|Uuid::parse_str(&id)).transpose().map_err(|_|rejected("invalid run"))? })
        })
    }
}

fn review_available(output: Option<&str>) -> bool {
    #[cfg(feature = "sandbox")]
    {
        output.is_some_and(|o| crate::coder::checker::ReviewPackage::from_output(o).is_ok())
    }
    #[cfg(not(feature = "sandbox"))]
    {
        let _ = output;
        false
    }
}

fn display_checker_output(output: &str) -> String {
    let Some(receipt) = output
        .lines()
        .find_map(|l| l.strip_prefix("Independent checker verdict: "))
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
    else {
        return output.to_owned();
    };
    let review = &receipt["review"];
    let mut text = format!(
        "{}\n\n{}",
        match review["verdict"].as_str() {
            Some("pass") => "Snapshot check passed",
            Some("changes_required") => "Changes needed",
            _ => "Check inconclusive",
        },
        review["summary"].as_str().unwrap_or("No summary")
    );
    if let Some(findings) = review["findings"].as_array() {
        for finding in findings {
            text.push_str(&format!(
                "\n\n{}:{} — {}\n{}",
                finding["path"].as_str().unwrap_or(""),
                finding["line"],
                finding["message"].as_str().unwrap_or(""),
                finding["evidence"].as_str().unwrap_or("")
            ));
        }
    }
    text.push_str("\n\nThis verdict applies to the saved snapshot. Tests were reported by the coder’s host; the checker did not rerun them.");
    text
}

fn saved_coding_files(output: Option<&str>) -> Vec<SavedCodingFile> {
    #[cfg(feature = "sandbox")]
    {
        output
            .and_then(|o| crate::coder::checker::ReviewPackage::from_output(o).ok())
            .map(|p| {
                p.snapshot
                    .files
                    .into_iter()
                    .map(|f| SavedCodingFile {
                        path: f.path,
                        before: f.before,
                        after: f.after,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
    #[cfg(not(feature = "sandbox"))]
    {
        let _ = output;
        Vec::new()
    }
}

#[cfg(all(test, feature = "sandbox"))]
mod saved_file_tests {
    use super::*;
    use crate::coder::checker::{
        CaptureIdentity, ReviewFile, ReviewPackage, ReviewSnapshot, PACKAGE_MARKER,
    };
    use sha2::{Digest, Sha256};
    #[test]
    fn saved_files_require_unique_valid_snapshot_and_preserve_exact_bytes() {
        let snapshot = ReviewSnapshot {
            identity: CaptureIdentity {
                task_id: Uuid::new_v4(),
                agent_id: Uuid::new_v4(),
                original_task: "Write marker".into(),
            },
            base_commit: "a".repeat(40),
            files: vec![ReviewFile {
                path: "result.txt".into(),
                before: None,
                after: Some("ready\n".into()),
            }],
            coder_report: "done".into(),
            acceptance: crate::coder::AcceptanceOutcome::Unverified,
            model_id: None,
        };
        let package = ReviewPackage {
            digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&snapshot).unwrap())
            ),
            snapshot,
        };
        let output = format!(
            "{PACKAGE_MARKER}{}",
            serde_json::to_string(&package).unwrap()
        );
        let files = saved_coding_files(Some(&output));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "result.txt");
        assert_eq!(files[0].after.as_deref(), Some("ready\n"));
        assert!(saved_coding_files(Some(&format!("{output}\n{output}"))).is_empty());
        assert!(saved_coding_files(Some(&output.replace("ready", "changed"))).is_empty());
        assert!(saved_coding_files(Some("model claims files exist")).is_empty());
    }
}
