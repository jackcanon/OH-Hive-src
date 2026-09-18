//! Selected primary and owner-approved LAN pairing. Independent of community server/tunnel state.
use crate::{HiveError, HiveNode, RUNTIME};
use hive_core::{
    local_hub::{
        authority::RemoteAuthoritySelection,
        enrollment::{
            verify_assertion, EnrollmentAssertion, EnrollmentChallenge, EnrollmentReceipt,
            EnrollmentTrust,
        },
        serve, RemoteLocalHub,
    },
    nodeconfig,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{oneshot, Mutex};
pub(crate) fn selected_wire() -> Result<Option<String>, HiveError> {
    match std::fs::read_to_string(nodeconfig::path().with_file_name("private-primary.json")) {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(fail(
            "Cannot read saved primary settings. Local fallback is disabled.",
        )),
    }
}
fn save_selection(path: &std::path::Path, wire: &str) -> Result<(), HiveError> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| fail("Invalid primary settings path"))?;
    std::fs::create_dir_all(parent).map_err(|_| fail("Cannot create primary settings folder"))?;
    let temporary = parent.join(format!(".private-primary-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(wire.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result.map_err(|_| fail("Cannot save primary settings"))
}
fn fail(message: &str) -> HiveError {
    HiveError::Failed(message.into())
}
#[derive(Default)]
pub(crate) struct FleetState {
    pub(crate) gate: Mutex<()>,
    pub(crate) private_stop: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    server: Mutex<Option<PrimaryServer>>,
    pending: Mutex<Option<PendingEnrollment>>,
}
struct PrimaryServer {
    address: String,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), hive_core::hub::HubError>>,
}
#[derive(Clone)]
struct PendingEnrollment {
    endpoint: String,
    key: String,
    challenge: EnrollmentChallenge,
}
#[derive(Clone, uniffi::Record)]
pub struct PrivatePrimaryStatus {
    pub mode: String,
    pub endpoint: Option<String>,
    pub connected: bool,
    pub detail: String,
}
pub(crate) fn selected() -> Result<Option<(RemoteAuthoritySelection, String)>, HiveError> {
    let Some(wire) = selected_wire()? else {
        return Ok(None);
    };
    let selection = serde_json::from_str(&wire).map_err(|_| fail("Saved primary settings are invalid. Reconnect your primary; local fallback is disabled."))?;
    Ok(Some((selection, wire)))
}
fn trust() -> Result<EnrollmentTrust, HiveError> {
    let get = |name| {
        nodeconfig::get_extra(name)
            .ok_or_else(|| fail("Private Fleet sign-in is not configured on this installation yet"))
    };
    Ok(EnrollmentTrust {
        issuer: get("HIVE_PRIVATE_FLEET_ISSUER")?,
        key_id: get("HIVE_PRIVATE_FLEET_KEY_ID")?,
        public_key: get("HIVE_PRIVATE_FLEET_PUBLIC_KEY")?,
    })
}
fn expected_receipt(
    pending: &PendingEnrollment,
    assertion: &EnrollmentAssertion,
) -> Result<EnrollmentReceipt, HiveError> {
    let claims = verify_assertion(&trust()?, assertion).map_err(HiveError::from)?;
    let ch = &pending.challenge;
    if claims.authority_id != ch.authority_id
        || claims.node_id != ch.node_id
        || claims.credential_sha256 != ch.credential_sha256
        || claims.nonce != ch.nonce
    {
        return Err(fail("Approval belongs to a different connection request"));
    }
    Ok(EnrollmentReceipt {
        authority_id: claims.authority_id,
        fleet_id: claims.fleet_id,
        owner_id: claims.subject,
        node_id: claims.node_id,
    })
}
#[uniffi::export]
impl HiveNode {
    pub fn private_primary_endpoint(&self) -> Result<Option<String>, HiveError> {
        Ok(selected()?.map(|(s, _)| s.endpoint))
    }
    pub async fn private_primary_status(
        self: Arc<Self>,
    ) -> Result<PrivatePrimaryStatus, HiveError> {
        RUNTIME.spawn(async move {
            if let Some((selection, _)) = selected()? {
                let connected = selection.connect().await.is_ok();
                return Ok(PrivatePrimaryStatus { mode: "secondary".into(), endpoint: Some(selection.endpoint), connected, detail: if connected { "Connected to your selected primary. Start this Mac’s coding worker to accept its assigned tasks." } else { "Primary unavailable. Your selected primary is unchanged; messages are not sent to a local copy." }.into() });
            }
            let mut guard = self.fleet.server.lock().await;
            if guard.as_ref().is_some_and(|s| s.task.is_finished()) { *guard = None; }
            Ok(PrivatePrimaryStatus { mode: "local".into(), endpoint: guard.as_ref().map(|s| format!("http://{}", s.address)), connected: guard.is_some(), detail: if guard.is_some() { "This Mac is accepting connections from your private network." } else { "Using this Mac's local history. Start sharing to connect another computer." }.into() })
        }).await.map_err(|_| fail("Primary status stopped"))?
    }
    /// Bind only an explicit private/loopback IP. Never starts the community server or tunnel.
    pub async fn private_primary_start(self: Arc<Self>, address: String) -> Result<(), HiveError> {
        RUNTIME.spawn(async move {
                let _operation = self.fleet.gate.lock().await;
            if selected()?.is_some() { return Err(fail("This computer is a secondary. Primary transfer is not available yet.")); }
            let address: SocketAddr = address.parse().map_err(|_| fail("Enter this Mac's private IP and port, for example 192.168.1.10:8787"))?;
            let private = match address.ip() { std::net::IpAddr::V4(ip) => ip.is_private() || ip.is_loopback() || ip.is_link_local(), std::net::IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local() };
            if !private || address.port() == 0 { return Err(fail("Choose a specific private network address and a nonzero port")); }
            let mut guard = self.fleet.server.lock().await;
            if guard.as_ref().is_some_and(|s| !s.task.is_finished()) { return Err(fail("Private sharing is already running. Stop it before changing its address.")); }
            let node = self.clone();
            let store = RUNTIME.spawn_blocking(move || node.private_bots_context()?.map(|v| v.0).ok_or_else(|| fail("Verify this Mac's Private Fleet identity first"))).await.map_err(|_| fail("Cannot open primary"))??;
            let listener = tokio::net::TcpListener::bind(address).await.map_err(|_| fail("Cannot listen at that address. Check this Mac's IP and whether the port is in use."))?;
            let (stop, rx) = oneshot::channel();
            let task = RUNTIME.spawn(serve(store, listener, async { let _ = rx.await; }));
            *guard = Some(PrimaryServer { address: address.to_string(), stop, task });
            Ok(())
        }).await.map_err(|_| fail("Starting private sharing stopped"))?
    }
    pub async fn private_primary_stop(self: Arc<Self>) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _operation = self.fleet.gate.lock().await;
                if let Some(server) = self.fleet.server.lock().await.take() {
                    let _ = server.stop.send(());
                    server
                        .task
                        .await
                        .map_err(|_| fail("Private sharing stopped unexpectedly"))?
                        .map_err(HiveError::from)?;
                }
                Ok(())
            })
            .await
            .map_err(|_| fail("Stopping private sharing failed"))?
    }
    pub async fn private_primary_pairing_code(self: Arc<Self>) -> Result<String, HiveError> {
        RUNTIME
            .spawn(async move {
                if !self
                    .fleet
                    .server
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|s| !s.task.is_finished())
                {
                    return Err(fail("Start private sharing first"));
                }
                RUNTIME
                    .spawn_blocking(move || {
                        self.private_primary_store()?
                            .pairing_code()
                            .map_err(HiveError::from)
                    })
                    .await
                    .map_err(|_| fail("Cannot create pairing code"))?
            })
            .await
            .map_err(|_| fail("Pairing code request stopped"))?
    }
    pub async fn private_primary_join_begin(
        self: Arc<Self>,
        endpoint: String,
        code: String,
        name: String,
    ) -> Result<String, HiveError> {
        RUNTIME
            .spawn(async move {
                let _operation = self.fleet.gate.lock().await;
                trust()?;
                let mut guard = self.fleet.pending.lock().await;
                if self
                    .fleet
                    .server
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|s| !s.task.is_finished())
                {
                    return Err(fail(
                        "Stop sharing this Mac before connecting to another primary",
                    ));
                }
                let credentials = RemoteLocalHub::pair(&endpoint, &code, &name)
                    .await
                    .map_err(HiveError::from)?;
                let remote = RemoteLocalHub::new(&endpoint, credentials.raw_key.clone())
                    .map_err(HiveError::from)?;
                let challenge = remote
                    .enrollment_challenge()
                    .await
                    .map_err(HiveError::from)?;
                let result = serde_json::to_string(&challenge)
                    .map_err(|_| fail("Cannot create connection request"))?;
                *guard = Some(PendingEnrollment {
                    endpoint,
                    key: credentials.raw_key,
                    challenge,
                });
                Ok(result)
            })
            .await
            .map_err(|_| fail("Connecting to primary stopped"))?
    }
    pub async fn private_primary_join_complete(
        self: Arc<Self>,
        approval: String,
    ) -> Result<(), HiveError> {
        RUNTIME
            .spawn(async move {
                let _operation = self.fleet.gate.lock().await;
                if approval.len() > 10000 {
                    return Err(fail("Invalid approval"));
                }
                let assertion: EnrollmentAssertion =
                    serde_json::from_str(&approval).map_err(|_| fail("Invalid approval"))?;
                let mut guard = self.fleet.pending.lock().await;
                let pending = guard
                    .as_ref()
                    .ok_or_else(|| fail("Create a connection request first"))?;
                if self
                    .fleet
                    .server
                    .lock()
                    .await
                    .as_ref()
                    .is_some_and(|s| !s.task.is_finished())
                {
                    return Err(fail(
                        "Stop sharing this Mac before selecting another primary",
                    ));
                }
                let expected = expected_receipt(pending, &assertion)?;
                let remote = RemoteLocalHub::new(&pending.endpoint, pending.key.clone())
                    .map_err(HiveError::from)?;
                // If the authority accepted an earlier attempt but its response was lost, confirm
                // the exact approved binding. Never replace this with an unverified whoami identity.
                let complete = remote.enrollment_complete(assertion).await;
                if let Err(error) = complete {
                    if remote.private_fleet_identity().await.is_err() {
                        return Err(HiveError::from(error));
                    }
                }
                let selection = RemoteAuthoritySelection::confirm(
                    pending.endpoint.clone(),
                    pending.key.clone(),
                    expected,
                )
                .await
                .map_err(HiveError::from)?;
                let wire = serde_json::to_string(&selection)
                    .map_err(|_| fail("Cannot save primary selection"))?;
                save_selection(
                    &nodeconfig::path().with_file_name("private-primary.json"),
                    &wire,
                )?;
                *guard = None;
                Ok(())
            })
            .await
            .map_err(|_| fail("Primary approval stopped"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_replacement_is_complete_and_private() {
        let folder = std::env::temp_dir().join(format!("hive-primary-{}", uuid::Uuid::new_v4()));
        let path = folder.join("private-primary.json");
        save_selection(&path, "first-fixture").unwrap();
        save_selection(&path, "replacement-fixture").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "replacement-fixture"
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
}
