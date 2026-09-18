use super::*;
use crate::bots::AgentBio;
use rusqlite::OptionalExtension;

// Uploaded images are pixels, never paths or URLs. Decode with explicit memory bounds,
// then encode a fresh PNG so metadata and ancillary content are not propagated.
fn normalize_avatar(value: &str) -> Result<String> {
    use base64::Engine;
    const PREFIX: &str = "data:image/png;base64,";
    let builtins = [
        "", "baldr", "bragi", "eir", "forseti", "freyja", "freyr", "frigg", "heimdall", "hel",
        "hodr", "idunn", "loki", "njord", "odin", "sif", "skadi", "thor", "tyr", "ullr", "vali",
        "vidar",
    ];
    if builtins.contains(&value) {
        return Ok(value.into());
    }
    let invalid = || rejected("Choose a built-in avatar or upload an image again");
    if value.len() > 512 * 1024 {
        return Err(invalid());
    }
    let encoded = value.strip_prefix(PREFIX).ok_or_else(invalid)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| invalid())?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: 2 * 1024 * 1024,
    });
    let mut reader = decoder.read_info().map_err(|_| invalid())?;
    let info = reader.info();
    if info.width == 0
        || info.height == 0
        || info.width > 256
        || info.height > 256
        || info.bit_depth != png::BitDepth::Eight
        || !matches!(info.color_type, png::ColorType::Rgb | png::ColorType::Rgba)
        || info.animation_control.is_some()
    {
        return Err(invalid());
    }
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).map_err(|_| invalid())?;
    let mut clean = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut clean, frame.width, frame.height);
        encoder.set_color(frame.color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| invalid())?;
        writer
            .write_image_data(&pixels[..frame.buffer_size()])
            .map_err(|_| invalid())?;
    }
    Ok(format!(
        "{}{}",
        PREFIX,
        base64::engine::general_purpose::STANDARD.encode(clean)
    ))
}

impl LocalHubStore {
    pub fn bots_agent_was_archived(
        &self,
        owner: Uuid,
        runtime: String,
        host: Option<Uuid>,
    ) -> Result<bool> {
        self.transaction(|tx| tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_profiles WHERE owner=?1 AND runtime_kind=?2 AND archived=1 AND (?3 IS NULL OR preferred_host=?3))", rusqlite::params![owner.to_string(),runtime,host.map(|h|h.to_string())], |r|r.get(0)).map_err(db_error))
    }

    pub fn bots_agent_bio_get(&self, owner: Uuid, agent: Uuid) -> Result<AgentBio> {
        self.transaction(|tx| {
            let actual: Option<String> = tx
                .query_row(
                    "SELECT owner FROM agent_profiles WHERE id=?1",
                    [agent.to_string()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_error)?;
            if actual.as_deref() != Some(owner.to_string().as_str()) {
                return Err(rejected("forbidden: agent owner"));
            }
            Ok(tx
                .query_row(
                    "SELECT bio,instructions,avatar,revision FROM bots_agent_bios WHERE agent=?1",
                    [agent.to_string()],
                    |r| {
                        Ok(AgentBio {
                            bio: r.get(0)?,
                            instructions: r.get(1)?,
                            avatar: r.get(2)?,
                            revision: r.get(3)?,
                        })
                    },
                )
                .optional()
                .map_err(db_error)?
                .unwrap_or_default())
        })
    }
    pub fn bots_agent_bio_set(
        &self,
        owner: Uuid,
        agent: Uuid,
        name: String,
        mut profile: AgentBio,
    ) -> Result<AgentBio> {
        let name = name.trim();
        profile.bio = profile.bio.trim().into();
        profile.instructions = profile.instructions.trim().into();
        profile.avatar = normalize_avatar(&profile.avatar)?;
        if name.is_empty()
            || name.len() > 200
            || name.chars().any(char::is_control)
            || profile.bio.len() > 4000
            || profile.instructions.len() > 16000
            || [&profile.bio, &profile.instructions]
                .iter()
                .any(|v| v.chars().any(|c| c.is_control() && c != '\n' && c != '\t'))
        {
            return Err(rejected("Use a name up to 200 bytes, bio up to 4000 bytes, instructions up to 16000 bytes and an available avatar"));
        }
        self.transaction(|tx| {
            let actual: Option<String> = tx.query_row("SELECT owner FROM agent_profiles WHERE id=?1 AND archived=0", [agent.to_string()], |r| r.get(0)).optional().map_err(db_error)?;
            if actual.as_deref()!=Some(owner.to_string().as_str()) { return Err(rejected("forbidden: active agent owner")); }
            let revision: u32 = tx.query_row("SELECT revision FROM bots_agent_bios WHERE agent=?1", [agent.to_string()], |r| r.get(0)).optional().map_err(db_error)?.unwrap_or(0);
            if profile.revision!=revision { return Err(rejected("Profile changed on another computer. Reload before saving.")); }
            profile.revision = revision.checked_add(1).ok_or_else(|| rejected("Profile revision exhausted"))?;
            tx.execute("INSERT INTO bots_agent_bios VALUES(?1,?2,?3,?4,?5) ON CONFLICT(agent) DO UPDATE SET bio=excluded.bio,instructions=excluded.instructions,avatar=excluded.avatar,revision=excluded.revision", rusqlite::params![agent.to_string(),profile.bio,profile.instructions,profile.avatar,profile.revision]).map_err(db_error)?;
            tx.execute("UPDATE agent_profiles SET name=?2,role_revision=role_revision+1,updated_at=?3 WHERE id=?1", rusqlite::params![agent.to_string(),name,now()]).map_err(db_error)?;
            Ok(profile)
        })
    }
}
impl LocalHub {
    pub fn bots_agent_was_archived(&self, runtime: String, host: Option<Uuid>) -> Result<bool> {
        self.store
            .bots_agent_was_archived(self.bots_owner()?, runtime, host)
    }

    pub fn bots_agent_bio_get(&self, agent: Uuid) -> Result<AgentBio> {
        self.store.bots_agent_bio_get(self.bots_owner()?, agent)
    }
    pub fn bots_agent_bio_set(
        &self,
        agent: Uuid,
        name: String,
        profile: AgentBio,
    ) -> Result<AgentBio> {
        self.store
            .bots_agent_bio_set(self.bots_owner()?, agent, name, profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::*;
    fn test_avatar(width: u32) -> String {
        use base64::Engine;
        let mut bytes=Vec::new();
        {
            let mut encoder=png::Encoder::new(&mut bytes,width,1);
            encoder.set_color(png::ColorType::Rgba);encoder.set_depth(png::BitDepth::Eight);
            encoder.add_text_chunk("Comment".into(),"Private source metadata".into()).unwrap();
            encoder.write_header().unwrap().write_image_data(&vec![128; width as usize * 4]).unwrap();
        }
        format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(bytes))
    }
    #[test]
    fn uploaded_avatar_is_bounded_decoded_and_stripped_of_metadata() {
        use base64::Engine;
        let clean=normalize_avatar(&test_avatar(2)).unwrap();
        assert_eq!(normalize_avatar(&clean).unwrap(),clean);
        let bytes=base64::engine::general_purpose::STANDARD.decode(clean.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        let reader=png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        assert!(reader.info().uncompressed_latin1_text.is_empty());
        assert!(normalize_avatar(&test_avatar(257)).is_err());
        assert!(normalize_avatar("data:image/png;base64,aGVsbG8=").is_err());
        assert!(normalize_avatar(&"x".repeat(512*1024+1)).is_err());
        assert!(normalize_avatar("https://example.com/avatar.png").is_err());
    }

    #[tokio::test]
    async fn agent_bio_remote_ownership_conflicts_and_archive() {
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
        let owner = Uuid::new_v4();
        store.set_node_owner(a.node_id, owner).unwrap();
        store.set_node_owner(b.node_id, Uuid::new_v4()).unwrap();
        let ca = RemoteLocalHub::new(&url, a.raw_key).unwrap();
        let cb = RemoteLocalHub::new(&url, b.raw_key).unwrap();
        let agent = store
            .bots_agents_create(NewAgentProfile {
                owner,
                name: "Before".into(),
                runtime_kind: AgentRuntimeKind::Local,
                preferred_host: Some(a.node_id),
                capability_policy_ref: "default".into(),
                provider_account_ref: None,
                memory_namespace: "test".into(),
            })
            .unwrap();
        assert_eq!(
            ca.bots_agent_bio_get(agent.id).await.unwrap(),
            AgentBio::default()
        );
        let bio = AgentBio {
            bio: "Research assistant".into(),
            instructions: "Cite sources".into(),
            avatar: "sif".into(),
            revision: 0,
        };
        assert!(cb.bots_agent_bio_get(agent.id).await.is_err());
        assert!(cb
            .bots_agent_bio_set(agent.id, "Intruder".into(), bio.clone())
            .await
            .is_err());
        let saved = ca
            .bots_agent_bio_set(agent.id, "Sif".into(), bio.clone())
            .await
            .unwrap();
        assert_eq!(saved.revision, 1);
        assert!(saved.prompt_context().contains("Cite sources"));
        assert_eq!(store.bots_agent_bio_get(owner, agent.id).unwrap(), saved);
        assert!(ca
            .bots_agent_bio_set(agent.id, "Stale".into(), bio)
            .await
            .is_err());
        let mut invalid = saved.clone();
        invalid.avatar = "../../secret".into();
        assert!(ca
            .bots_agent_bio_set(agent.id, "Bad".into(), invalid)
            .await
            .is_err());
        let mut invalid = saved.clone();
        invalid.instructions = "x".repeat(16001);
        assert!(ca
            .bots_agent_bio_set(agent.id, "Bad".into(), invalid)
            .await
            .is_err());
        let agents = ca.bots_agents_list().await.unwrap();
        assert_eq!(agents[0].name, "Sif");
        assert_eq!(agents[0].role_revision, agent.role_revision + 1);
        assert!(cb.bots_agents_archive(agent.id).await.is_err());
        let mut custom = saved.clone(); custom.avatar = test_avatar(2);
        let saved = ca.bots_agent_bio_set(agent.id,"Sif".into(),custom).await.unwrap();
        assert!(saved.avatar.starts_with("data:image/png;base64,"));
        assert_eq!(ca.bots_agent_bio_get(agent.id).await.unwrap(),saved);
        ca.bots_agents_archive(agent.id).await.unwrap();
        assert!(ca.bots_agents_list().await.unwrap().is_empty());
        assert!(ca
            .bots_agent_was_archived("local".into(), Some(a.node_id))
            .await
            .unwrap());
        assert!(!cb
            .bots_agent_was_archived("local".into(), Some(a.node_id))
            .await
            .unwrap());
        assert_eq!(ca.bots_agent_bio_get(agent.id).await.unwrap(), saved);
        assert!(ca
            .bots_agent_bio_set(agent.id, "After deletion".into(), saved)
            .await
            .is_err());
        server.abort();
    }
}
