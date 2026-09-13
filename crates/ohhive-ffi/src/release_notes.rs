//! FFI wrapper for release notes (#178, 2026-09-13 -- Jack, mid-session: "we also need to make
//! sure when we are updating now that we have a couple of users we need to make sure when users
//! login after an update there should be release notes"). Same node-key-only constraint as
//! `chat.rs`/`channel.rs`: this app never holds a member Supabase session, so both calls go
//! through the node-key-authenticated `hive_release_notes_unseen_node`/
//! `hive_release_notes_mark_seen_node` RPCs (migration 20260913080000), which resolve the owning
//! member from the paired device key server-side, same as everywhere else in this crate.

use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::hub::{HubClient, ReleaseNote as HubReleaseNote};
use hive_core::nodeconfig;
use std::sync::Arc;

#[derive(uniffi::Record, Clone)]
pub struct ReleaseNote {
    pub seq: i64,
    pub version: String,
    pub title: String,
    pub body_md: String,
    pub published_at: String,
}

impl From<HubReleaseNote> for ReleaseNote {
    fn from(n: HubReleaseNote) -> Self {
        Self {
            seq: n.seq,
            version: n.version,
            title: n.title,
            body_md: n.body_md,
            published_at: n.published_at,
        }
    }
}

#[uniffi::export]
impl HiveNode {
    /// Whatever release notes this machine's owning member hasn't seen yet, oldest first --
    /// the app shows these once at launch (a `.sheet`, per `ContentView`) if the list isn't
    /// empty, then calls `releaseNotesMarkSeen()` once the member dismisses it.
    pub async fn release_notes_unseen(self: Arc<Self>) -> Result<Vec<ReleaseNote>, HiveError> {
        let cfg = nodeconfig::load().map_err(HiveError::from)?;
        let key = cfg
            .node_key
            .clone()
            .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        let notes = RUNTIME
            .spawn(async move { hub.release_notes_unseen().await })
            .await
            .map_err(|e| HiveError::Failed(format!("release_notes_unseen task panicked: {e}")))?
            .map_err(HiveError::from)?;
        Ok(notes.into_iter().map(Into::into).collect())
    }

    /// Acknowledges every release note published so far -- the member won't be shown this dialog
    /// again on any machine until a new note is published.
    pub async fn release_notes_mark_seen(self: Arc<Self>) -> Result<(), HiveError> {
        let cfg = nodeconfig::load().map_err(HiveError::from)?;
        let key = cfg
            .node_key
            .clone()
            .ok_or_else(|| HiveError::Failed("pair this machine first".into()))?;
        let hub = HubClient::new(&cfg.hub_url, &cfg.anon_key, key);
        RUNTIME
            .spawn(async move { hub.release_notes_mark_seen().await })
            .await
            .map_err(|e| {
                HiveError::Failed(format!("release_notes_mark_seen task panicked: {e}"))
            })?
            .map_err(HiveError::from)
    }
}
