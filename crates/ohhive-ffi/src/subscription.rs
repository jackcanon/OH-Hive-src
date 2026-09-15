use crate::{HiveError, HiveNode, RUNTIME};
use std::sync::Arc;

#[derive(Clone, uniffi::Record)]
pub struct ChatGptAccountStatus {
    pub state: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub auth_url: Option<String>,
    pub user_code: Option<String>,
    pub detail: String,
}

#[uniffi::export]
impl HiveNode {
    pub async fn chatgpt_account(
        self: Arc<Self>,
        action: String,
        binary: Option<String>,
    ) -> Result<ChatGptAccountStatus, HiveError> {
        let status = RUNTIME
            .spawn(async move {
                hive_core::subscription::account::account_action(&action, binary).await
            })
            .await
            .map_err(|_| HiveError::Failed("Account service stopped".into()))?;
        Ok(ChatGptAccountStatus {
            state: status.state,
            email: status.email,
            plan: status.plan,
            auth_url: status.auth_url,
            user_code: status.user_code,
            detail: status.detail,
        })
    }
}
