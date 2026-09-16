//! FFI wrapper for submitting a feature request or bug report from the Swift app (Jack,
//! 2026-09-12/2026-09-13: members want a way to suggest features and report bugs from both the
//! website and the desktop app). Same node-key-only constraint as `kanban.rs`'s cloud-projects
//! read: this app never holds a member Supabase session, only a node key, so submission goes
//! through `hub.rs`'s `HubClient::submit_feature_request`/`submit_bug_report`
//! (`public.hive_feature_request_create_node`/`hive_bug_report_create_node`, migrations
//! 20260912200000/20260913000000) rather than the member-authenticated path the web app's
//! /requests page uses. Browsing/voting/comments/attachments stay a web-only feature for now
//! (that's a richer, list-shaped UI that fits a website better) -- this is just the "send one in
//! from wherever you are" path, for both kinds of feedback.

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::hub::HubClient;
use hive_core::nodeconfig;
use std::sync::Arc;

#[uniffi::export]
impl HiveNode {
    pub async fn submit_feature_request(
        self: Arc<Self>,
        title: String,
        description: String,
    ) -> Result<(), HiveError> {
        self.log(
            "info",
            format!("submitting feature request: \u{201c}{title}\u{201d}"),
        )
        .await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
                hub.submit_feature_request(&title, &description)
                    .await
                    .map_err(HiveError::from)?;
                Ok(())
            })
            .await
            .map_err(|e| HiveError::Failed(format!("submit_feature_request task panicked: {e}")))?;
        match &r {
            Ok(()) => this.log("ok", "feature request submitted -- thanks!").await,
            Err(e) => {
                this.log("error", format!("feature request failed: {e}"))
                    .await
            }
        }
        r
    }

    pub async fn submit_bug_report(
        self: Arc<Self>,
        title: String,
        description: String,
        anonymous: bool,
    ) -> Result<(), HiveError> {
        self.log(
            "info",
            format!("submitting bug report: \u{201c}{title}\u{201d}"),
        )
        .await;
        let this = self.clone();
        let r = RUNTIME
            .spawn(async move {
                let cfg = nodeconfig::load().map_err(HiveError::from)?;
                let key = cfg
                    .node_key
                    .clone()
                    .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
                let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
                hub.submit_bug_report(&title, &description, anonymous)
                    .await
                    .map_err(HiveError::from)?;
                Ok(())
            })
            .await
            .map_err(|e| HiveError::Failed(format!("submit_bug_report task panicked: {e}")))?;
        match &r {
            Ok(()) => this.log("ok", "bug report submitted -- thanks!").await,
            Err(e) => this.log("error", format!("bug report failed: {e}")).await,
        }
        r
    }
}
