use super::*;
use crate::bots::UserProfile;
use rusqlite::OptionalExtension;
impl LocalHubStore {
    pub fn bots_conversation_deliveries(
        &self,
        owner: uuid::Uuid,
        conversation: uuid::Uuid,
    ) -> Result<Vec<(String, String, String)>> {
        if self.bots_conversation_get(conversation)?.owner != owner {
            return Err(rejected(
                "forbidden: conversation belongs to another account",
            ));
        }
        self.transaction(|tx| {
            let mut q = tx.prepare("SELECT d.message_id,a.name,d.status FROM agent_deliveries d JOIN messages m ON m.id=d.message_id JOIN agent_profiles a ON a.id=d.recipient WHERE m.conversation_id=?1 ORDER BY m.server_sequence DESC,a.name LIMIT 500").map_err(db_error)?;
            let rows = q.query_map([conversation.to_string()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db_error)?;
            rows.collect::<std::result::Result<Vec<_>,_>>().map_err(db_error)
        })
    }

    pub fn bots_user_profile_get(&self, owner: uuid::Uuid) -> Result<UserProfile> {
        self.transaction(|tx| {
            tx.query_row(
                "SELECT preferred_name,about FROM bots_user_profiles WHERE owner=?1",
                [owner.to_string()],
                |r| {
                    Ok(UserProfile {
                        preferred_name: r.get(0)?,
                        about: r.get(1)?,
                    })
                },
            )
            .optional()
            .map(|p| p.unwrap_or_default())
            .map_err(db_error)
        })
    }
    pub fn bots_user_profile_set(
        &self,
        owner: uuid::Uuid,
        mut profile: UserProfile,
    ) -> Result<UserProfile> {
        profile.preferred_name = profile.preferred_name.trim().to_string();
        profile.about = profile.about.trim().to_string();
        if profile.preferred_name.len() > 120
            || profile.about.len() > 2000
            || profile.preferred_name.chars().any(char::is_control)
            || profile
                .about
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(rejected(
                "Use a name up to 120 bytes and background up to 2000 bytes",
            ));
        }
        self.transaction(|tx| {
            tx.execute("INSERT INTO bots_user_profiles VALUES(?1,?2,?3) ON CONFLICT(owner) DO UPDATE SET preferred_name=excluded.preferred_name,about=excluded.about", rusqlite::params![owner.to_string(), profile.preferred_name, profile.about]).map_err(db_error)?;
            Ok(profile)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_profile_roundtrip_limits_and_clear() {
        let store = LocalHubStore::in_memory().unwrap();
        let owner = uuid::Uuid::new_v4();
        let other = uuid::Uuid::new_v4();
        assert_eq!(
            store.bots_user_profile_get(owner).unwrap(),
            UserProfile::default()
        );
        let profile = UserProfile {
            preferred_name: " Jack ".into(),
            about: "I produce shows.".into(),
        };
        assert_eq!(
            store
                .bots_user_profile_set(owner, profile)
                .unwrap()
                .preferred_name,
            "Jack"
        );
        assert_eq!(
            store.bots_user_profile_get(other).unwrap(),
            UserProfile::default()
        );
        assert!(store
            .bots_user_profile_set(
                owner,
                UserProfile {
                    preferred_name: "x".repeat(121),
                    about: "".into()
                }
            )
            .is_err());
        assert_eq!(
            store.bots_user_profile_get(owner).unwrap().preferred_name,
            "Jack"
        );
        assert!(store
            .bots_user_profile_set(
                owner,
                UserProfile {
                    preferred_name: "Jack".into(),
                    about: "x".repeat(2001)
                }
            )
            .is_err());
        store
            .bots_user_profile_set(owner, UserProfile::default())
            .unwrap();
        assert_eq!(
            store.bots_user_profile_get(owner).unwrap(),
            UserProfile::default()
        );
    }
    #[tokio::test]
    async fn user_profile_transport_scopes_to_verified_owner() {
        let store = LocalHubStore::in_memory().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve(store.clone(), listener, std::future::pending()));
        let a = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "A")
            .await
            .unwrap();
        let b = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "B")
            .await
            .unwrap();
        let ca = RemoteLocalHub::new(&url, a.raw_key).unwrap();
        let cb = RemoteLocalHub::new(&url, b.raw_key).unwrap();
        assert!(ca.bots_user_profile_get().await.is_err());
        let owner = uuid::Uuid::new_v4();
        store.set_node_owner(a.node_id, owner).unwrap();
        store
            .set_node_owner(b.node_id, uuid::Uuid::new_v4())
            .unwrap();
        ca.bots_user_profile_set(UserProfile {
            preferred_name: "Jack".into(),
            about: "Podcast producer".into(),
        })
        .await
        .unwrap();
        assert_eq!(
            cb.bots_user_profile_get().await.unwrap(),
            UserProfile::default()
        );
        let c = RemoteLocalHub::pair(&url, &store.pairing_code().unwrap(), "C")
            .await
            .unwrap();
        store.set_node_owner(c.node_id, owner).unwrap();
        let cc = RemoteLocalHub::new(&url, c.raw_key).unwrap();
        let conversation = store
            .bots_conversations_create(crate::bots::NewConversation {
                title: Some("Test room".into()),
                owner,
                kind: crate::bots::ConversationKind::Team,
                project_id: None,
                coordinator: None,
                storage_scope: crate::bots::StorageScope::LocalOnly,
            })
            .unwrap();
        assert!(ca
            .bots_conversation_deliveries(conversation.id)
            .await
            .unwrap()
            .is_empty());
        assert!(cc
            .bots_conversation_deliveries(conversation.id)
            .await
            .unwrap()
            .is_empty());
        assert!(cb
            .bots_conversation_deliveries(conversation.id)
            .await
            .is_err());
        let p = cc.bots_user_profile_get().await.unwrap();
        assert_eq!(p.preferred_name, "Jack");
        assert!(p.prompt_context().contains("Never call them Owner"));
        assert!(p.prompt_context().contains("Podcast producer"));
        server.abort();
    }
}
