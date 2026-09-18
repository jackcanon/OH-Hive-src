//! The model chooses only a tool and arguments; the executor supplies all authority.
use super::{failed, LocalTurnError, LocalTurnOutcome, LocalTurnRequest, TurnUsage};
use crate::{
    backend::llama_cpp::{
        LlamaCppBackend, ToolChatMessage, ToolChatResult, ToolFunctionSchema, ToolSchema,
    },
    local_hub::{
        agent_tools::{AgentToolCall, AgentToolPolicy, AgentToolTurn},
        LocalHub, RemoteLocalHub,
    },
};
use uuid::Uuid;
#[derive(Clone)]
pub enum LibraryToolHost {
    Local(LocalHub),
    Remote(RemoteLocalHub),
}
impl LibraryToolHost {
    pub async fn policy(&self, agent: Uuid) -> Result<AgentToolPolicy, crate::hub::HubError> {
        match self {
            Self::Local(h) => h.bots_agent_tool_policy_get(agent),
            Self::Remote(h) => h.bots_agent_tool_policy_get(agent).await,
        }
    }
    async fn execute(
        &self,
        agent: Uuid,
        revision: u32,
        turn: &AgentToolTurn,
        call: AgentToolCall,
    ) -> Result<serde_json::Value, crate::hub::HubError> {
        match self {
            Self::Local(h) => h.bots_agent_tool_execute(agent, revision, turn, call),
            Self::Remote(h) => h.bots_agent_tool_execute(agent, revision, turn, call).await,
        }
    }
}
fn message(role: &str, content: String) -> ToolChatMessage {
    ToolChatMessage {
        role: role.into(),
        content: Some(content),
        tool_calls: None,
        tool_call_id: None,
    }
}
fn schemas(vaults: &[Uuid]) -> Vec<ToolSchema> {
    let scope = serde_json::json!({"type":"string","enum":vaults});
    [("vault_search","Search selected library documents",serde_json::json!({"type":"object","properties":{"vault":scope,"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["vault","query","limit"],"additionalProperties":false})),
    ("vault_read","Read a document at the revision returned by search",serde_json::json!({"type":"object","properties":{"vault":scope,"document":{"type":"string","format":"uuid","description":"The exact id UUID from a vault_search hit. Use id, not the document path."},"revision":{"type":"string","description":"The exact revision from the same vault_search hit."}},"required":["vault","document","revision"],"additionalProperties":false}))]
    .into_iter().map(|(name,description,parameters)|ToolSchema {kind:"function".into(),function:ToolFunctionSchema{name:name.into(),description:description.into(),parameters}}).collect()
}
pub(super) async fn run(
    backend: &LlamaCppBackend,
    model: &str,
    host: &LibraryToolHost,
    agent: Uuid,
    request: &LocalTurnRequest,
    policy: AgentToolPolicy,
    prompt: String,
) -> Result<LocalTurnOutcome, LocalTurnError> {
    if request.delivery_generation == 0 {
        return Err(failed("Library tools require an active chat attempt"));
    }
    if backend
        .model_tool_support(model)
        .await
        .map_err(|_| failed("Cannot check model tool support"))?
        == Some(false)
    {
        return Err(failed(
            "Selected model does not support library tools. Choose a tool-capable model.",
        ));
    }
    let turn = AgentToolTurn {
        message: request.incoming.id,
        conversation: request.conversation_id,
        generation: request.delivery_generation,
        conversation_revision: request.conversation_policy_revision,
    };
    let mut messages = vec![message("system", prompt)];
    let tools = schemas(&policy.readable_vaults);
    let mut used = 0;
    let mut usage = TurnUsage::default();
    for _ in 0..9 {
        let encoded = serde_json::to_vec(&messages).map_err(|_| failed("Invalid tool context"))?;
        if encoded.len() > 256 * 1024 {
            return Err(failed("Library context limit reached"));
        }
        let (result, tokens) = backend
            .chat_with_tools(model, &messages, &tools, 2048)
            .await
            .map_err(|_| failed("Local model library request failed"))?;
        usage.prompt_tokens = usage.prompt_tokens.saturating_add(tokens.tokens_in);
        usage.completion_tokens = usage.completion_tokens.saturating_add(tokens.tokens_out);
        match result {
            ToolChatResult::Text(text) => {
                if text.trim().is_empty() || text.len() > 65536 {
                    return Err(failed("Invalid library reply size"));
                }
                return Ok(LocalTurnOutcome {
                    reply_body: text.trim().into(),
                    usage: Some(usage),
                });
            }
            ToolChatResult::ToolCalls(mut calls) => {
                if used + calls.len() > 8 {
                    return Err(failed("Chat library tool limit reached"));
                }
                for (i, call) in calls.iter_mut().enumerate() {
                    if call.function.arguments.len() > 8192 || call.kind != "function" {
                        return Err(failed("Invalid library tool request"));
                    }
                    call.id = format!("library-{}", used + i);
                }
                messages.push(ToolChatMessage {
                    role: "assistant".into(),
                    content: None,
                    tool_calls: Some(calls.clone()),
                    tool_call_id: None,
                });
                for call in calls {
                    let mut args: serde_json::Value =
                        serde_json::from_str(&call.function.arguments)
                            .map_err(|_| failed("Invalid library arguments"))?;
                    let object = args
                        .as_object_mut()
                        .ok_or_else(|| failed("Invalid library arguments"))?;
                    if object.contains_key("tool") {
                        return Err(failed("Unexpected library argument"));
                    }
                    object.insert("tool".into(), call.function.name.into());
                    let parsed: AgentToolCall = serde_json::from_value(args)
                        .map_err(|_| failed("Unsupported library tool or arguments"))?;
                    let result=host.execute(agent,policy.revision,&turn,parsed).await.map_err(|_|failed("Library access or chat attempt changed. Review access and try again."))?;
                    let content = serde_json::to_string(&result)
                        .map_err(|_| failed("Invalid library result"))?;
                    if content.len() > 65536 {
                        return Err(failed("Library result limit reached"));
                    }
                    messages.push(ToolChatMessage {
                        role: "tool".into(),
                        content: Some(content),
                        tool_calls: None,
                        tool_call_id: Some(call.id),
                    });
                    used += 1;
                }
            }
        }
    }
    Err(failed("Chat library tool limit reached"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bots::*,
        local_hub::{agent_tools::test_turn, LocalHubStore},
    };
    use axum::{routing::post, Json, Router};
    #[tokio::test]
    async fn library_model_loop_reads_real_scoped_content_and_stops_after_cancel() {
        for cancel in [false, true] {
            let s = LocalHubStore::in_memory().unwrap();
            let c = s.enroll_owner("host").unwrap();
            let owner = Uuid::new_v4();
            s.set_node_owner(c.node_id, owner).unwrap();
            let a = s
                .bots_agents_create(NewAgentProfile {
                    owner,
                    name: "Researcher".into(),
                    runtime_kind: AgentRuntimeKind::Local,
                    preferred_host: Some(c.node_id),
                    capability_policy_ref: "none".into(),
                    provider_account_ref: None,
                    memory_namespace: "test".into(),
                })
                .unwrap();
            let turn = test_turn(&s, &a);
            let v = s.vault_create("Library").unwrap();
            let doc = Uuid::new_v4();
            let rev = s
                .vault_put(v, doc, "answer.md", "Evidence", "The answer is forty-two.")
                .unwrap();
            s.vault_set_available(v, true).unwrap();
            s.vault_grant(v, c.node_id, true).unwrap();
            let h = s.connect(&c.raw_key).unwrap();
            let p = h
                .bots_agent_tool_policy_set(
                    a.id,
                    AgentToolPolicy {
                        readable_vaults: vec![v],
                        ..Default::default()
                    },
                )
                .unwrap();
            let request = LocalTurnRequest {
                conversation_id: turn.conversation,
                delivery_generation: turn.generation,
                conversation_policy_revision: turn.conversation_revision,
                history: vec![],
                incoming: s.bots_message_get(turn.message).unwrap(),
                speakers: vec![],
                participants_note: String::new(),
            };
            let store = s.clone();
            let message_id = turn.message;
            let agent_id = a.id;
            let app=Router::new().route("/api/show",post(||async{Json(serde_json::json!({"capabilities":["tools","completion"]}))})).route("/v1/chat/completions",post(move |Json(body):Json<serde_json::Value>|{
                let rev=rev.clone();let store=store.clone();async move {
                    assert_eq!(body["tools"].as_array().unwrap().len(),2);
                    let messages=body["messages"].as_array().unwrap();
                    if messages.len()==1 {
                        if cancel {store.bots_delivery_cancel(Principal::User(owner),DeliveryKey{message_id,recipient:agent_id}).unwrap();}
                        Json(serde_json::json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"read","type":"function","function":{"name":"vault_read","arguments":serde_json::json!({"vault":v,"document":doc,"revision":rev}).to_string()}}]}}]}))
                    } else {
                        assert!(!cancel,"cancelled delivery must not return library data to the model");
                        let result:serde_json::Value=serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
                        assert_eq!(result["result"]["content"],"The answer is forty-two.");
                        Json(serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":"Forty-two, from answer.md."}}]}))
                    }
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let backend = LlamaCppBackend::local_only(&format!("http://{address}")).unwrap();
            let result = run(
                &backend,
                "test",
                &LibraryToolHost::Local(h),
                a.id,
                &request,
                p,
                "Read the library".into(),
            )
            .await;
            if cancel {
                assert!(result.is_err());
            } else {
                assert_eq!(result.unwrap().reply_body, "Forty-two, from answer.md.");
            }
            server.abort();
        }
    }
}
