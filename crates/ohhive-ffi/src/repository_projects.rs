//! Primary-local project administration. Never silently opens a secondary project database.
use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::local_hub::{repository::ProjectRepository, LocalHubStore};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, uniffi::Record)]
pub struct PrivateRepositoryProject {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub repo_url: Option<String>,
    pub repo_ref: Option<String>,
}

impl HiveNode {
    fn repository_project_store(&self) -> Result<LocalHubStore, HiveError> {
        if crate::private_fleet::selected()?.is_some() {
            return Err(HiveError::Failed("Manage project repositories on your selected primary computer. Remote project editing is not available yet.".into()));
        }
        self.private_bots_context()?.map(|v| v.0).ok_or_else(|| {
            HiveError::Failed(
                "Sign in to Private Fleet on this computer before creating projects.".into(),
            )
        })
    }
}

#[uniffi::export]
impl HiveNode {
    pub async fn private_repository_projects(
        self: Arc<Self>,
    ) -> Result<Vec<PrivateRepositoryProject>, HiveError> {
        RUNTIME
            .spawn(async move {
                let _operation = self.fleet.gate.lock().await;
                let node = self.clone();
                RUNTIME
                    .spawn_blocking(move || {
                        Ok(node
                            .repository_project_store()?
                            .repository_projects()
                            .map_err(HiveError::from)?
                            .into_iter()
                            .map(|p| PrivateRepositoryProject {
                                id: p.id.to_string(),
                                title: p.title,
                                goal: p.goal,
                                repo_url: p.repository.as_ref().map(|r| r.repo_url.clone()),
                                repo_ref: p.repository.and_then(|r| r.repo_ref),
                            })
                            .collect())
                    })
                    .await
                    .map_err(|_| HiveError::Failed("Project loading stopped".into()))?
            })
            .await
            .map_err(|_| HiveError::Failed("Project loading stopped".into()))?
    }

    pub async fn private_repository_project_create(
        self: Arc<Self>,
        title: String,
        goal: String,
    ) -> Result<String, HiveError> {
        RUNTIME
            .spawn(async move {
                let _operation = self.fleet.gate.lock().await;
                let node = self.clone();
                RUNTIME
                    .spawn_blocking(move || {
                        Ok(node
                            .repository_project_store()?
                            .create_project(&title, &goal)
                            .map_err(HiveError::from)?
                            .to_string())
                    })
                    .await
                    .map_err(|_| HiveError::Failed("Project creation stopped".into()))?
            })
            .await
            .map_err(|_| HiveError::Failed("Project creation stopped".into()))?
    }

    pub async fn private_repository_project_set(
        self: Arc<Self>,
        project_id: String,
        repo_url: Option<String>,
        repo_ref: Option<String>,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _operation = self.fleet.gate.lock().await;
                let node = self.clone();
                RUNTIME
                    .spawn_blocking(move || {
                        let id = Uuid::parse_str(&project_id)
                            .map_err(|_| HiveError::Failed("Invalid project identity".into()))?;
                        let binding =
                            repo_url.map(|repo_url| ProjectRepository { repo_url, repo_ref });
                        node.repository_project_store()?
                            .set_project_repository(id, binding.as_ref())
                            .map_err(HiveError::from)
                    })
                    .await
                    .map_err(|_| HiveError::Failed("Repository update stopped".into()))?
            })
            .await
            .map_err(|_| HiveError::Failed("Repository update stopped".into()))?
    }
}
