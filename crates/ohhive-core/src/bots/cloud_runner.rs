//! Explicit BYOK-only hub turns. Credentials are node credentials, never provider keys.
use super::{
    AgentProfile, AgentRuntimeKind, LocalBotsTurnRunner, LocalTurnError, LocalTurnOutcome,
    LocalTurnRequest, TurnUsage, UserId,
};
use async_trait::async_trait;
use futures::StreamExt;
use std::time::Duration;

// Intentionally no Debug implementation: this object holds a node credential.
pub struct CloudTurnRunner {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    owner: UserId,
    anon_key: String,
    raw_key: String,
}
fn failed(s: &str) -> LocalTurnError {
    LocalTurnError::RuntimeFailed(s.into())
}
impl CloudTurnRunner {
    /// Use the trusted hub configuration and the authenticated owner of this node.
    /// Do not construct from agent-authored URLs. Private-only enrollment is not supported
    /// by this community BYOK endpoint yet. No Local runtime can use this runner.
    pub fn new(
        hub_url: &str,
        anon_key: String,
        raw_key: String,
        owner: UserId,
    ) -> Result<Self, LocalTurnError> {
        let mut endpoint = reqwest::Url::parse(hub_url).map_err(|_| failed("Invalid hub URL"))?;
        if endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/"
            || anon_key.is_empty()
            || raw_key.is_empty()
            || raw_key.len() > 4096
        {
            return Err(failed(
                "A trusted HTTPS hub and node credentials are required",
            ));
        }
        endpoint.set_path("/functions/v1/bots-turn");
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(110))
            .build()
            .map_err(|_| failed("Cannot initialize cloud runner"))?;
        Ok(Self {
            client,
            endpoint,
            owner,
            anon_key,
            raw_key,
        })
    }
    fn payload(
        &self,
        agent: &AgentProfile,
        request: LocalTurnRequest,
    ) -> Result<serde_json::Value, LocalTurnError> {
        let provider = match agent.runtime_kind {
            AgentRuntimeKind::AnthropicByok => "anthropic",
            AgentRuntimeKind::NousByok => "nous",
            _ => return Err(failed("Agent is not a BYOK cloud agent")),
        };
        if agent.archived
            || agent.owner != self.owner
            || agent.name.trim().is_empty()
            || agent.name.len() > 512
            || request.history.len() > 64
            || request.participants_note.len() > 8192
            || request.speakers.len() > 256
            || request.speakers.iter().any(|(_, n)| n.len() > 512)
        {
            return Err(failed("Invalid cloud agent or context"));
        }
        if request
            .incoming
            .body
            .as_deref()
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(failed("Empty incoming message"));
        }
        let mut size = 0usize;
        let mut messages = Vec::new();
        for m in request
            .history
            .iter()
            .chain(std::iter::once(&request.incoming))
        {
            let text = m.body.as_deref().unwrap_or("");
            size = size.saturating_add(text.len());
            if m.conversation_id != request.conversation_id || size > 65536 {
                return Err(failed("Invalid or oversized conversation context"));
            }
            let speaker = request
                .speakers
                .iter()
                .find(|(p, _)| p == &m.author)
                .map(|(_, n)| n.as_str())
                .unwrap_or("an unnamed participant");
            messages.push(serde_json::json!({"speaker": speaker, "text": text}));
        }
        let value = serde_json::json!({"owner_id":self.owner,"provider":provider,"agent_name":agent.name,"participants_note":request.participants_note,"messages":messages,"raw_key":self.raw_key});
        if serde_json::to_vec(&value)
            .map_err(|_| failed("Invalid context"))?
            .len()
            > 128 * 1024
        {
            return Err(failed("Encoded context is too large"));
        }
        Ok(value)
    }
    async fn execute(
        &self,
        agent: &AgentProfile,
        request: LocalTurnRequest,
    ) -> Result<LocalTurnOutcome, LocalTurnError> {
        let body = self.payload(agent, request)?;
        let response = self
            .client
            .post(self.endpoint.clone())
            .header("apikey", &self.anon_key)
            .bearer_auth(&self.anon_key)
            .json(&body)
            .send()
            .await
            .map_err(|_| failed("Cloud reply service is unavailable or timed out"))?;
        if response.status().as_u16() == 429 {
            return Err(LocalTurnError::NoCapacity);
        }
        if !response.status().is_success() {
            return Err(failed(match response.status().as_u16() {
                401 | 403 => "Cloud account authorization failed",
                409 => "Add a key for this provider in Settings",
                _ => "Cloud reply service failed",
            }));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| failed("Cloud reply was interrupted"))?;
            if bytes.len().saturating_add(chunk.len()) > 256 * 1024 {
                return Err(failed("Cloud reply exceeded its size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        decode(&bytes)
    }
}
fn decode(bytes: &[u8]) -> Result<LocalTurnOutcome, LocalTurnError> {
    #[derive(serde::Deserialize)]
    struct Usage {
        prompt_tokens: u64,
        completion_tokens: u64,
    }
    #[derive(serde::Deserialize)]
    struct Reply {
        reply_body: String,
        usage: Option<Usage>,
    }
    let reply: Reply = serde_json::from_slice(bytes).map_err(|_| failed("Invalid cloud reply"))?;
    if reply.reply_body.trim().is_empty() || reply.reply_body.len() > 65536 {
        return Err(failed("Invalid cloud reply length"));
    }
    Ok(LocalTurnOutcome {
        reply_body: reply.reply_body.trim().into(),
        usage: reply.usage.map(|u| TurnUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        }),
    })
}
#[async_trait]
impl LocalBotsTurnRunner for CloudTurnRunner {
    async fn run_turn(
        &self,
        agent: &AgentProfile,
        request: LocalTurnRequest,
    ) -> Result<LocalTurnOutcome, LocalTurnError> {
        tokio::time::timeout(Duration::from_secs(115), self.execute(agent, request))
            .await
            .map_err(|_| failed("Cloud turn timed out"))?
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (CloudTurnRunner, AgentProfile, LocalTurnRequest) {
        let id = uuid::Uuid::new_v4();
        let owner = uuid::Uuid::new_v4();
        let now = chrono::Utc::now();
        let agent = AgentProfile {
            id,
            owner,
            name: "Nous".into(),
            role_revision: 1,
            runtime_kind: AgentRuntimeKind::NousByok,
            preferred_host: None,
            capability_policy_ref: "no-tools".into(),
            provider_account_ref: None,
            memory_namespace: "private".into(),
            archived: false,
            created_at: now,
            updated_at: now,
        };
        let conversation_id = uuid::Uuid::new_v4();
        let incoming = crate::bots::Message {
            id: uuid::Uuid::new_v4(),
            conversation_id,
            thread_root: None,
            author: crate::bots::Principal::User(owner),
            server_sequence: 1,
            client_request_id: "test".into(),
            kind: crate::bots::MessageKind::Text,
            body: Some("Hello".into()),
            attachment_refs: vec![],
            task_ref: None,
            turn_ref: None,
            source_event_ref: None,
            created_at: now,
        };
        let runner =
            CloudTurnRunner::new("https://example.com", "anon".into(), "node".into(), owner)
                .unwrap();
        let request = LocalTurnRequest {
            conversation_id,
            history: vec![],
            incoming,
            speakers: vec![(crate::bots::Principal::User(owner), "Jack".into())],
            participants_note: "".into(),
        };
        (runner, agent, request)
    }
    #[test]
    fn local_and_other_owner_never_send() {
        let (r, mut a, q) = fixture();
        a.runtime_kind = AgentRuntimeKind::Local;
        assert!(r.payload(&a, q).is_err());
        let (r, mut a, q) = fixture();
        a.owner = uuid::Uuid::new_v4();
        assert!(r.payload(&a, q).is_err());
        let (r, a, mut q) = fixture();
        q.incoming.conversation_id = uuid::Uuid::new_v4();
        assert!(r.payload(&a, q).is_err());
        let (r, a, q) = fixture();
        let p = r.payload(&a, q).unwrap();
        assert_eq!(p["messages"][0]["speaker"], "Jack");
        assert_eq!(p["provider"], "nous");
    }
    #[tokio::test]
    async fn http_rate_limit_error_and_success() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, body) in [
            (429, "secret"),
            (500, "secret"),
            (200, r#"{"reply_body":"Hello","usage":null}"#),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 8192];
                let _ = stream.read(&mut buf).await.unwrap();
                let response = format!(
                    "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
            let (mut r, a, q) = fixture();
            r.endpoint = format!("http://{address}/").parse().unwrap();
            let result = r.run_turn(&a, q).await;
            match status {
                429 => assert!(matches!(result, Err(LocalTurnError::NoCapacity))),
                200 => assert_eq!(result.unwrap().reply_body, "Hello"),
                _ => assert!(
                    matches!(result, Err(LocalTurnError::RuntimeFailed(s)) if !s.contains("secret"))
                ),
            }
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn stalled_http_reply_times_out() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let (mut r, a, q) = fixture();
        r.endpoint = format!("http://{address}/").parse().unwrap();
        r.client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let result = r.run_turn(&a, q).await;
        server.abort();
        assert!(matches!(result, Err(LocalTurnError::RuntimeFailed(s)) if s.contains("timed out")));
    }
    #[tokio::test]
    async fn already_cancelled_sends_nothing() {
        let (r, a, q) = fixture();
        let (_sender, receiver) = tokio::sync::watch::channel(true);
        assert!(matches!(
            r.run_turn_cancellable(&a, q, receiver).await,
            Err(LocalTurnError::Cancelled)
        ));
    }
    #[test]
    fn reject_unsafe_hub_urls() {
        for url in [
            "http://example.com",
            "https://user:pass@example.com",
            "https://example.com/other",
            "https://example.com/?token=x",
        ] {
            assert!(
                CloudTurnRunner::new(url, "anon".into(), "node".into(), uuid::Uuid::new_v4())
                    .is_err()
            );
        }
    }
    #[test]
    fn reply_contract_and_limits() {
        assert_eq!(
            decode(
                br#"{"reply_body":" hello ","usage":{"prompt_tokens":1,"completion_tokens":2}}"#
            )
            .unwrap()
            .reply_body,
            "hello"
        );
        for v in [
            serde_json::json!({"reply_body":" "}),
            serde_json::json!({"reply_body":"x".repeat(65537)}),
            serde_json::json!({"reply_body":"ok","usage":{"prompt_tokens":-1,"completion_tokens":2}}),
        ] {
            assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
        }
    }
}
