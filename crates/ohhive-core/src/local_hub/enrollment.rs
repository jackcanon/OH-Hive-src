//! Private enrollment: verified platform identity is independent of community membership.
//! Only trusted local administration installs issuer keys. RPC callers cannot choose trust.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::signature::{UnparsedPublicKey, ED25519};

const DOMAIN: &str = "hive.private-fleet.enrollment.v1\n";
const TTL: i64 = 300;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentTrust {
    pub issuer: String,
    pub key_id: String,
    pub public_key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentChallenge {
    pub authority_id: Uuid,
    pub node_id: Uuid,
    pub credential_sha256: String,
    pub nonce: String,
    pub expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentAssertion {
    pub payload: String,
    pub signature: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentClaims {
    pub issuer: String,
    pub key_id: String,
    pub audience: String,
    pub subject: Uuid,
    pub fleet_id: Uuid,
    pub authority_id: Uuid,
    pub node_id: Uuid,
    pub credential_sha256: String,
    pub nonce: String,
    pub assertion_id: Uuid,
    pub issued_at: i64,
    pub expires_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnrollmentReceipt {
    pub authority_id: Uuid,
    pub fleet_id: Uuid,
    pub owner_id: Uuid,
    pub node_id: Uuid,
}

pub fn verify_assertion(trust: &EnrollmentTrust, envelope: &EnrollmentAssertion) -> Result<EnrollmentClaims> {
    if envelope.payload.len() > 8192 || envelope.signature.len() > 128 {
        return Err(rejected("invalid enrollment assertion"));
    }
    let key = URL_SAFE_NO_PAD
        .decode(&trust.public_key)
        .map_err(|_| rejected("invalid enrollment trust key"))?;
    let signature = URL_SAFE_NO_PAD
        .decode(&envelope.signature)
        .map_err(|_| rejected("invalid enrollment signature"))?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(
            format!("{DOMAIN}{}", envelope.payload).as_bytes(),
            &signature,
        )
        .map_err(|_| rejected("invalid enrollment signature"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(&envelope.payload)
        .map_err(|_| rejected("invalid enrollment assertion"))?;
    let c: EnrollmentClaims =
        serde_json::from_slice(&payload).map_err(|_| rejected("invalid enrollment claims"))?;
    let time = now();
    if c.issuer != trust.issuer
        || c.key_id != trust.key_id
        || c.audience != "hive-private-fleet-enrollment"
        || c.subject.is_nil()
        || c.fleet_id.is_nil()
        || c.assertion_id.is_nil()
        || c.issued_at > time + 30
        || c.expires_at <= time
        || c.expires_at <= c.issued_at
        || c.expires_at.saturating_sub(c.issued_at) > TTL
        || c.nonce.len() != 64
        || c.credential_sha256.len() != 64
    {
        return Err(rejected("enrollment assertion has invalid scope or expiry"));
    }
    Ok(c)
}

impl LocalHubStore {
    /// Trusted bootstrap only. Trust must come from configured platform issuer metadata,
    /// never from the assertion/joining client. No HTTP method exposes this operation.
    pub fn configure_private_fleet(
        &self,
        trust: EnrollmentTrust,
        assertion: EnrollmentAssertion,
        local_key: &str,
    ) -> Result<EnrollmentReceipt> {
        let url = reqwest::Url::parse(&trust.issuer)
            .map_err(|_| rejected("invalid enrollment issuer"))?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            return Err(rejected("enrollment issuer must use HTTPS"));
        }
        let claims = verify_assertion(&trust, &assertion)?;
        self.connect(local_key)?
            .accept_enrollment(claims, Some(trust))
    }
}
impl LocalHub {
    /// Offline identity from the authority's persisted, verified enrollment.
    /// An unconfigured authority is distinct from an unbound/revoked device.
    pub fn private_fleet_identity(&self) -> Result<Option<EnrollmentReceipt>> {
        self.with_node(|tx, node| {
            let (authority, fleet, owner): (String, Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT authority_id,fleet_id,owner_id FROM private_fleet_authority WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(db_error)?;
            let Some(fleet) = fleet else {
                return Ok(None);
            };
            let bound: Option<String> = tx
                .query_row(
                    "SELECT owner_member_id FROM nodes WHERE id=?1",
                    [node],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if bound.is_none() || bound != owner {
                return Err(rejected("device enrollment is required"));
            }
            let parse = |s: &str| {
                Uuid::parse_str(s).map_err(|_| rejected("invalid private fleet identity"))
            };
            Ok(Some(EnrollmentReceipt {
                authority_id: parse(&authority)?,
                fleet_id: parse(&fleet)?,
                owner_id: parse(&owner.unwrap())?,
                node_id: parse(node)?,
            }))
        })
    }
    pub fn enrollment_challenge(&self) -> Result<EnrollmentChallenge> {
        self.with_node(|tx, node| {
            let authority: String = tx.query_row("SELECT authority_id FROM private_fleet_authority WHERE id=1", [], |r| r.get(0)).map_err(db_error)?;
            let nonce: String = (0..32).map(|_| format!("{:02x}", OsRng.gen::<u8>())).collect();
            let expires_at = now() + TTL;
            tx.execute("INSERT INTO private_fleet_challenges VALUES(?1,?2,?3,?4) ON CONFLICT(node_id) DO UPDATE SET key_hash=excluded.key_hash,nonce=excluded.nonce,expires_at=excluded.expires_at", params![node,self.key_hash,nonce,expires_at]).map_err(db_error)?;
            Ok(EnrollmentChallenge { authority_id: Uuid::parse_str(&authority).map_err(|_| rejected("invalid authority"))?, node_id: Uuid::parse_str(node).map_err(|_| HubError::BadKey)?, credential_sha256: self.key_hash.clone(), nonce, expires_at })
        })
    }
    pub fn enrollment_complete(&self, assertion: EnrollmentAssertion) -> Result<EnrollmentReceipt> {
        let trust: EnrollmentTrust = self.with_node(|tx, _| {
            let trust: Option<String> = tx
                .query_row(
                    "SELECT trust FROM private_fleet_authority WHERE id=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            decode(&trust.ok_or_else(|| {
                rejected("private fleet authority must be configured by its owner first")
            })?)
        })?;
        self.accept_enrollment(verify_assertion(&trust, &assertion)?, None)
    }
    fn accept_enrollment(
        &self,
        c: EnrollmentClaims,
        bootstrap: Option<EnrollmentTrust>,
    ) -> Result<EnrollmentReceipt> {
        self.with_node(|tx, node| {
            let (authority, fleet, owner): (String,Option<String>,Option<String>) = tx.query_row("SELECT authority_id,fleet_id,owner_id FROM private_fleet_authority WHERE id=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_error)?;
            if authority != c.authority_id.to_string() || node != c.node_id.to_string() || self.key_hash != c.credential_sha256 {
                return Err(rejected("enrollment assertion targets another authority or credential"));
            }
            if let Some(fleet) = fleet {
                if bootstrap.is_some() || fleet != c.fleet_id.to_string() || owner.as_deref() != Some(c.subject.to_string().as_str()) {
                    return Err(rejected("enrollment assertion belongs to another fleet or owner"));
                }
            } else if bootstrap.is_none() { return Err(rejected("private fleet is not configured")); }
            let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM private_fleet_challenges WHERE node_id=?1 AND key_hash=?2 AND nonce=?3 AND expires_at>?4)", params![node,self.key_hash,c.nonce,now()], |r|r.get(0)).map_err(db_error)?;
            if !valid || c.expires_at <= now() { return Err(rejected("enrollment challenge expired or already used")); }
            let existing: Option<String> = tx.query_row("SELECT owner_member_id FROM nodes WHERE id=?1", [node], |r|r.get(0)).map_err(db_error)?;
            if existing.is_some_and(|v| v != c.subject.to_string()) { return Err(rejected("node is already bound to another account")); }
            tx.execute("INSERT INTO private_fleet_enrollments VALUES(?1,?2,?3)",params![c.assertion_id.to_string(),node,now()]).map_err(|_|rejected("enrollment assertion already used"))?;
            tx.execute("UPDATE nodes SET owner_member_id=?2 WHERE id=?1",params![node,c.subject.to_string()]).map_err(db_error)?;
            if let Some(trust) = bootstrap {
                tx.execute("UPDATE private_fleet_authority SET fleet_id=?1,owner_id=?2,trust=?3 WHERE id=1",params![c.fleet_id.to_string(),c.subject.to_string(),encode(&trust)?]).map_err(db_error)?;
            }
            tx.execute("DELETE FROM private_fleet_challenges WHERE node_id=?1",[node]).map_err(db_error)?;
            Ok(EnrollmentReceipt {authority_id:c.authority_id,fleet_id:c.fleet_id,owner_id:c.subject,node_id:c.node_id})
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    fn signer() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap()
    }
    fn trust() -> EnrollmentTrust {
        EnrollmentTrust {
            issuer: "https://identity.example/enroll".into(),
            key_id: "test-1".into(),
            public_key: URL_SAFE_NO_PAD.encode(signer().public_key().as_ref()),
        }
    }
    fn claims(ch: &EnrollmentChallenge, owner: Uuid, fleet: Uuid) -> EnrollmentClaims {
        EnrollmentClaims {
            issuer: trust().issuer,
            key_id: trust().key_id,
            audience: "hive-private-fleet-enrollment".into(),
            subject: owner,
            fleet_id: fleet,
            authority_id: ch.authority_id,
            node_id: ch.node_id,
            credential_sha256: ch.credential_sha256.clone(),
            nonce: ch.nonce.clone(),
            assertion_id: Uuid::new_v4(),
            issued_at: now(),
            expires_at: now() + 120,
        }
    }
    fn sign(c: &EnrollmentClaims) -> EnrollmentAssertion {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(c).unwrap());
        let signature = URL_SAFE_NO_PAD.encode(
            signer()
                .sign(format!("{DOMAIN}{payload}").as_bytes())
                .as_ref(),
        );
        EnrollmentAssertion { payload, signature }
    }
    fn configured() -> (LocalHubStore, Uuid, Uuid) {
        let store = LocalHubStore::in_memory().unwrap();
        let key = store.enroll_owner("primary").unwrap();
        let hub = store.connect(&key.raw_key).unwrap();
        let ch = hub.enrollment_challenge().unwrap();
        let owner = Uuid::new_v4();
        let fleet = Uuid::new_v4();
        assert!(hub
            .enrollment_complete(sign(&claims(&ch, owner, fleet)))
            .is_err());
        store
            .configure_private_fleet(trust(), sign(&claims(&ch, owner, fleet)), &key.raw_key)
            .unwrap();
        (store, owner, fleet)
    }
    #[test]
    fn signed_enrollment_rejects_each_mismatched_scope_without_consuming_challenge() {
        let (store, owner, fleet) = configured();
        let key = store.enroll_owner("secondary").unwrap();
        let hub = store.connect(&key.raw_key).unwrap();
        let good = claims(&hub.enrollment_challenge().unwrap(), owner, fleet);
        for field in 0..13 {
            let mut bad = good.clone();
            match field {
                0 => bad.subject = Uuid::new_v4(),
                1 => bad.fleet_id = Uuid::new_v4(),
                2 => bad.authority_id = Uuid::new_v4(),
                3 => bad.node_id = Uuid::new_v4(),
                4 => bad.credential_sha256 = "0".repeat(64),
                5 => bad.nonce = "0".repeat(64),
                6 => bad.issuer = "https://evil.example".into(),
                7 => bad.key_id = "another".into(),
                8 => bad.audience = "other-service".into(),
                9 => {
                    bad.issued_at = now() - 400;
                    bad.expires_at = now() - 100;
                }
                10 => {
                    bad.issued_at = now() + 60;
                    bad.expires_at = now() + 120;
                }
                11 => bad.expires_at = now() + 1000,
                _ => bad.assertion_id = Uuid::nil(),
            }
            assert!(
                hub.enrollment_complete(sign(&bad)).is_err(),
                "scope case {field}"
            );
        }
        let mut tampered = sign(&good);
        tampered.signature = URL_SAFE_NO_PAD.encode([0; 64]);
        assert!(hub.enrollment_complete(tampered).is_err());
        assert!(hub.private_fleet_identity().is_err());
        let receipt = hub.enrollment_complete(sign(&good)).unwrap();
        assert_eq!(
            hub.private_fleet_identity().unwrap().unwrap().owner_id,
            owner
        );
        assert_eq!(receipt.owner_id, owner);
        assert!(hub.enrollment_complete(sign(&good)).is_err());
        let mut reuse = claims(&hub.enrollment_challenge().unwrap(), owner, fleet);
        reuse.assertion_id = good.assertion_id;
        assert!(hub.enrollment_complete(sign(&reuse)).is_err());
        reuse.assertion_id = Uuid::new_v4();
        hub.enrollment_complete(sign(&reuse)).unwrap();
    }
    #[test]
    fn replacement_challenge_revocation_and_existing_binding_are_enforced() {
        let (store, owner, fleet) = configured();
        let key = store.enroll_owner("secondary").unwrap();
        let hub = store.connect(&key.raw_key).unwrap();
        let stale = claims(&hub.enrollment_challenge().unwrap(), owner, fleet);
        let current = claims(&hub.enrollment_challenge().unwrap(), owner, fleet);
        assert!(hub.enrollment_complete(sign(&stale)).is_err());
        store.set_node_owner(key.node_id, Uuid::new_v4()).unwrap();
        assert!(hub.enrollment_complete(sign(&current)).is_err());
        store.revoke(key.node_id).unwrap();
        assert!(matches!(
            hub.enrollment_complete(sign(&current)),
            Err(HubError::BadKey)
        ));
        assert!(hub.enrollment_challenge().is_err());
    }
    #[test]
    fn expired_authority_challenge_and_trust_replacement_are_rejected() {
        let (store, owner, fleet) = configured();
        let key = store.enroll_owner("secondary").unwrap();
        let hub = store.connect(&key.raw_key).unwrap();
        let c = claims(&hub.enrollment_challenge().unwrap(), owner, fleet);
        assert!(store
            .configure_private_fleet(trust(), sign(&c), &key.raw_key)
            .is_err());
        store
            .transaction(|tx| {
                tx.execute("UPDATE private_fleet_challenges SET expires_at=0", [])
                    .map_err(db_error)?;
                Ok(())
            })
            .unwrap();
        assert!(hub.enrollment_complete(sign(&c)).is_err());
    }
    #[test]
    fn authority_and_verified_identity_survive_restart() {
        let path = std::env::temp_dir().join(format!("hive-enrollment-{}.sqlite3", Uuid::new_v4()));
        let store = LocalHubStore::open(&path).unwrap();
        let key = store.enroll_owner("primary").unwrap();
        let hub = store.connect(&key.raw_key).unwrap();
        assert!(hub.private_fleet_identity().unwrap().is_none());
        let ch = hub.enrollment_challenge().unwrap();
        let owner = Uuid::new_v4();
        let fleet = Uuid::new_v4();
        store
            .configure_private_fleet(trust(), sign(&claims(&ch, owner, fleet)), &key.raw_key)
            .unwrap();
        drop(hub);
        drop(store);
        let reopened = LocalHubStore::open(&path).unwrap();
        let hub = reopened.connect(&key.raw_key).unwrap();
        assert_eq!(
            hub.private_fleet_identity().unwrap().unwrap().owner_id,
            owner
        );
        assert_eq!(
            hub.enrollment_challenge().unwrap().authority_id,
            ch.authority_id
        );
        reopened.revoke(key.node_id).unwrap();
        assert!(hub.private_fleet_identity().is_err());
        drop(hub);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[cfg(feature = "bots")]
    #[tokio::test]
    async fn remote_pairing_and_verified_enrollment_open_bots_without_community_membership() {
        let (store, owner, fleet) = configured();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (stop, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(super::super::transport::serve(
            store.clone(),
            listener,
            async {
                let _ = rx.await;
            },
        ));
        let key = super::super::transport::RemoteLocalHub::pair(
            &url,
            &store.pairing_code().unwrap(),
            "remote secondary",
        )
        .await
        .unwrap();
        let remote =
            super::super::transport::RemoteLocalHub::new(&url, key.raw_key.clone()).unwrap();
        assert!(remote.bots_agents_list().await.is_err());
        let assertion = sign(&claims(
            &remote.enrollment_challenge().await.unwrap(),
            owner,
            fleet,
        ));
        let receipt = remote.enrollment_complete(assertion.clone()).await.unwrap();
        assert_eq!(receipt.owner_id, owner);
        assert!(remote.bots_agents_list().await.unwrap().is_empty());
        assert_eq!(
            remote
                .private_fleet_identity()
                .await
                .unwrap()
                .unwrap()
                .owner_id,
            owner
        );
        assert!(remote.enrollment_complete(assertion).await.is_err());
        use super::super::authority::RemoteAuthoritySelection;
        let selected =
            RemoteAuthoritySelection::confirm(url.clone(), key.raw_key.clone(), receipt.clone())
                .await
                .unwrap();
        let persisted = serde_json::to_string(&selected).unwrap();
        let restored: RemoteAuthoritySelection = serde_json::from_str(&persisted).unwrap();
        let session = restored.connect().await.unwrap();
        assert!(session.agents().await.unwrap().is_empty());
        for field in 0..4 {
            let mut wrong = receipt.clone();
            match field {
                0 => wrong.fleet_id = Uuid::new_v4(),
                1 => wrong.owner_id = Uuid::new_v4(),
                2 => wrong.node_id = Uuid::new_v4(),
                _ => wrong.authority_id = Uuid::new_v4(),
            }
            assert!(
                RemoteAuthoritySelection::confirm(url.clone(), key.raw_key.clone(), wrong)
                    .await
                    .is_err()
            );
        }
        use crate::bots::*;
        let conversation = store
            .bots_conversations_create(NewConversation {
                    title: None,
                owner,
                kind: ConversationKind::Team,
                project_id: None,
                coordinator: None,
                storage_scope: StorageScope::LocalOnly,
            })
            .unwrap();
        let message = NewMessage {
            thread_root: None,
            kind: MessageKind::Text,
            body: Some("same primary, same message".into()),
            attachment_refs: vec![],
            task_ref: None,
            turn_ref: None,
            source_event_ref: None,
        };
        let first = session
            .send(
                conversation.id,
                "durable-request-1".into(),
                conversation.policy_revision,
                vec![],
                message.clone(),
            )
            .await
            .unwrap();
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
        assert!(restored.connect().await.is_err());
        assert!(session
            .send(
                conversation.id,
                "durable-request-1".into(),
                conversation.policy_revision,
                vec![],
                message.clone()
            )
            .await
            .is_err());
        let listener = tokio::net::TcpListener::bind(url.trim_start_matches("http://"))
            .await
            .unwrap();
        let (stop, rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(super::super::transport::serve(
            store.clone(),
            listener,
            async {
                let _ = rx.await;
            },
        ));
        let resumed = restored.connect().await.unwrap();
        let retry = resumed
            .send(
                conversation.id,
                "durable-request-1".into(),
                conversation.policy_revision,
                vec![],
                message,
            )
            .await
            .unwrap();
        assert_eq!(retry.id, first.id);
        assert_eq!(
            resumed
                .messages(
                    conversation.id,
                    MessagePage {
                        before: None,
                        after: None,
                        limit: 50
                    }
                )
                .await
                .unwrap()
                .len(),
            1
        );
        store.revoke(key.node_id).unwrap();
        assert!(remote.bots_agents_list().await.is_err());
        assert!(session.agents().await.is_err());
        assert!(restored.connect().await.is_err());
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
        assert!(restored.connect().await.is_err());
        assert!(session.agents().await.is_err());
    }
}
