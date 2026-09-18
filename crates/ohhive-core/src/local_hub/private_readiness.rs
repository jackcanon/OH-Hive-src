//! Host-reported metadata, never execution authorization or proof of repository access.
use super::private_code_tasks::{verified_owner, PrivateExecutionHost};
use super::*;
const TTL: i64 = 45;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingModel {
    pub id: String,
    pub supports_tools: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodingReadiness {
    pub worker_enabled: bool,
    pub coding_enabled: bool,
    pub git_connected: bool,
    pub models: Vec<CodingModel>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodingHost {
    pub host: PrivateExecutionHost,
    pub report: Option<CodingReadiness>,
    pub observed_at: Option<i64>,
    pub fresh: bool,
}
impl LocalHub {
    /// Identity and timestamp come from the authority, not report fields.
    pub fn private_coding_advertise(&self, report: &CodingReadiness) -> Result<()> {
        if report.models.len() > 64 {
            return Err(rejected("too many advertised coding models"));
        }
        let mut ids = std::collections::HashSet::new();
        for model in &report.models {
            check_text(&model.id, 500)?;
            if !ids.insert(&model.id) {
                return Err(rejected("duplicate coding model"));
            }
        }
        self.with_node(|tx,node| {
            verified_owner(tx,node)?;
            tx.execute("INSERT INTO private_coding_readiness VALUES(?1,?2,?3) ON CONFLICT(node_id) DO UPDATE SET report=excluded.report,observed_at=excluded.observed_at",params![node,encode(report)?,now()]).map_err(db_error)?;
            Ok(())
        })
    }
    pub fn private_coding_hosts(&self) -> Result<Vec<CodingHost>> {
        self.with_node(|tx,node| {
            let owner=verified_owner(tx,node)?;
            let mut q=tx.prepare("SELECT n.id,n.name,r.report,r.observed_at FROM nodes n LEFT JOIN private_coding_readiness r ON r.node_id=n.id WHERE n.owner_member_id=?1 AND EXISTS(SELECT 1 FROM private_fleet_enrollments e WHERE e.node_id=n.id) AND EXISTS(SELECT 1 FROM local_node_keys k WHERE k.node_id=n.id AND k.revoked=0) ORDER BY n.name,n.id").map_err(db_error)?;
            let rows=q.query_map([owner],|r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<i64>>(3)?))).map_err(db_error)?;
            let time=now();
            rows.map(|row| {
                let (id,name,report,observed_at)=row.map_err(db_error)?;
                Ok(CodingHost {
                    host:PrivateExecutionHost { node_id:Uuid::parse_str(&id).map_err(|_| rejected("invalid coding host identity"))?,name },
                    report:report.map(|s|decode(&s)).transpose()?,observed_at,
                    fresh:observed_at.is_some_and(|t|t<=time && t>time-TTL),
                })
            }).collect()
        })
    }
}

impl RemoteLocalHub {
    /// Metadata-only refresh on the target. Publication failure never starts work or switches host.
    #[cfg(feature = "llama-cpp")]
    pub async fn refresh_private_coding_readiness(
        &self,
        backend: &crate::backend::llama_cpp::LlamaCppBackend,
        worker_enabled: bool,
        coding_enabled: bool,
        git_connected: bool,
    ) -> Result<()> {
        use crate::backend::Backend;
        let mut report = CodingReadiness {
            worker_enabled,
            coding_enabled,
            git_connected,
            models: vec![],
        };
        // Clear earlier model availability immediately if the server is now unreachable.
        let caps = match backend.capabilities().await {
            Ok(caps) => caps,
            Err(_) => {
                self.private_coding_advertise(&report).await?;
                return Err(rejected(
                    "model server unavailable; model availability cleared",
                ));
            }
        };
        use futures::StreamExt;
        let discovery =
            futures::stream::iter(caps.models.into_iter().take(64).map(|model| async move {
                let supports_tools = backend.model_tool_support(&model.id).await.unwrap_or(None);
                CodingModel {
                    id: model.id.clone(),
                    supports_tools,
                }
            }))
            .buffered(8)
            .collect::<Vec<_>>();
        match tokio::time::timeout(std::time::Duration::from_secs(8), discovery).await {
            Ok(models) => report.models = models,
            Err(_) => {
                self.private_coding_advertise(&report).await?;
                return Err(rejected(
                    "model discovery timed out; refresh before choosing this host",
                ));
            }
        }
        self.private_coding_advertise(&report).await
    }
}
