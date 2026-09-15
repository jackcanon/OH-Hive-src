use super::*;
use crate::desktop::{Access, Authority, Decision, Denial, Policy, Risk, Target};
use std::sync::{Arc, Mutex};
fn screenshot() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 4, 3);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk("private_metadata".into(), "must not upload".into())
            .unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[255; 48]).unwrap();
    }
    bytes
}
fn observation() -> Observation {
    Observation {
        id: 7,
        target: Target {
            bundle_id: "test.app".into(),
            process_id: 42,
            process_instance: Uuid::new_v4(),
            window_id: 9,
        },
        width: 4,
        height: 3,
        allowed_rect: [0, 0, 4, 3],
        valid_until: 100,
    }
}
fn turn<'a>(png: &'a [u8], o: &'a Observation) -> Turn<'a> {
    Turn {
        task: "Click the button",
        png,
        observation: o,
        session: Uuid::new_v4(),
        call_id: Uuid::new_v4(),
        sequence: 3,
        policy_revision: 1,
        upload_authorized: true,
        now: 1,
    }
}
fn response(input: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"type":"message","role":"assistant","stop_reason":"tool_use","usage":{"input_tokens":100,"output_tokens":20},"content":[{"type":"tool_use","name":"computer","id":"toolu_test","input":input}]})).unwrap()
}
struct Mock {
    calls: Arc<Mutex<Vec<Value>>>,
    result: Result<Vec<u8>>,
}
#[async_trait]
impl Transport for Mock {
    async fn post(&self, key: &ApiKey, body: Value) -> Result<Vec<u8>> {
        assert!(key.0.is_sensitive());
        self.calls.lock().unwrap().push(body);
        match &self.result {
            Ok(bytes) => Ok(bytes.clone()),
            Err(ProviderError::Transport) => Err(ProviderError::Transport),
            Err(ProviderError::Http(status)) => Err(ProviderError::Http(*status)),
            _ => unreachable!(),
        }
    }
}
fn adapter(result: Result<Vec<u8>>) -> (AnthropicDesktop, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    (
        AnthropicDesktop {
            transport: Box::new(Mock {
                calls: calls.clone(),
                result,
            }),
            key: ApiKey::from_local_secret("synthetic-key-never-live").unwrap(),
            model: Model::Sonnet46,
        },
        calls,
    )
}
#[tokio::test]
async fn sends_only_computer_tool_and_strips_image_metadata() {
    let (provider, calls) = adapter(Ok(response(
        json!({"action":"left_click","coordinate":[1,2]}),
    )));
    let png = screenshot();
    let o = observation();
    let t = turn(&png, &o);
    let session = t.session;
    let call = t.call_id;
    let proposal = provider.propose(t).await.unwrap();
    let body = &calls.lock().unwrap()[0];
    assert_eq!(body["tools"].as_array().unwrap().len(), 1);
    assert_eq!(body["tools"][0]["type"], "computer_20251124");
    assert_eq!(body["model"], "claude-sonnet-4-6");
    assert_eq!(body["max_tokens"], 1024);
    assert!(!body.to_string().contains("synthetic-key-never-live"));
    assert!(!body
        .to_string()
        .contains(&o.target.process_instance.to_string()));
    let encoded = body["messages"][0]["content"][1]["source"]["data"]
        .as_str()
        .unwrap();
    let clean = STANDARD.decode(encoded).unwrap();
    assert!(!clean
        .windows(b"must not upload".len())
        .any(|s| s == b"must not upload"));
    let mut reader = png::Decoder::new(Cursor::new(clean)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut pixels).unwrap();
    assert_eq!(pixels, vec![255; 48]);
    let Proposal::Action { request, usage, .. } = proposal else {
        panic!("expected action")
    };
    assert_eq!(request.session, session);
    assert_eq!(request.call_id, call);
    assert_eq!(request.target, o.target);
    assert_eq!(request.observation, 7);
    assert_eq!(request.sequence, 3);
    assert_eq!(request.action, Action::Click { x: 1, y: 2 });
    assert_eq!(usage.output_tokens, 20);
    // The caller, not adapter, evaluates authority. A valid proposal is still denied view-only.
    let owner = Uuid::new_v4();
    let node = Uuid::new_v4();
    let a = Authority {
        session,
        owner,
        project_owner: owner,
        node,
        target_node: node,
        private_local: true,
        lease_deadline: 100,
        authority_deadline: 100,
        stopped: false,
        grant_deadline: 100,
        access: Access::View,
        target: o.target.clone(),
    };
    assert_eq!(
        crate::desktop::evaluate(&a, &Policy::default(), &o, &request, Risk::Routine, None, 1),
        Decision::Deny(Denial::Access)
    );
}
#[tokio::test]
async fn invalid_or_unapproved_screens_never_reach_transport() {
    let (provider, calls) = adapter(Ok(response(json!({"action":"screenshot"}))));
    let png = screenshot();
    let o = observation();
    let mut t = turn(&png, &o);
    t.upload_authorized = false;
    assert!(matches!(
        provider.propose(t).await,
        Err(ProviderError::UploadDenied)
    ));
    let mut t = turn(&png, &o);
    t.now = 100;
    assert!(provider.propose(t).await.is_err());
    let mut bad_o = o.clone();
    bad_o.width = 5;
    assert!(provider.propose(turn(&png, &bad_o)).await.is_err());
    bad_o = o.clone();
    bad_o.allowed_rect = [u32::MAX, 0, 4, 3];
    assert!(provider.propose(turn(&png, &bad_o)).await.is_err());
    assert!(provider
        .propose(turn(b"\x89PNG\r\n\x1a\n", &o))
        .await
        .is_err());
    assert!(provider
        .propose(turn(&vec![0; MAX_IMAGE + 1], &o))
        .await
        .is_err());
    assert!(calls.lock().unwrap().is_empty());
}
#[test]
fn only_supported_exact_action_shapes_are_accepted() {
    let png = screenshot();
    let o = observation();
    let t = turn(&png, &o);
    for input in [
        json!({"action":"screenshot"}),
        json!({"action":"type","text":"hello"}),
        json!({"action":"left_click","coordinate":[0,0]}),
    ] {
        assert!(parse_response(&response(input), &t).is_ok());
    }
    for input in [
        json!({"action":"key","text":"Return"}),
        json!({"action":"bash","command":"x"}),
        json!({"action":"left_click"}),
        json!({"action":"left_click","coordinate":[-1,0]}),
        json!({"action":"left_click","coordinate":[4,0]}),
        json!({"action":"left_click","coordinate":[0.1,0]}),
        json!({"action":"screenshot","grant":true}),
        json!({"action":"type","text":"x","target":"other"}),
        json!({"action":"type","text":"x".repeat(MAX_TEXT+1)}),
    ] {
        assert!(matches!(
            parse_response(&response(input), &t),
            Err(ProviderError::InvalidResponse)
        ));
    }
}
#[test]
fn rejects_batches_foreign_tools_truncation_and_malformed_bodies() {
    let png = screenshot();
    let o = observation();
    let t = turn(&png, &o);
    let original: Value =
        serde_json::from_slice(&response(json!({"action":"screenshot"}))).unwrap();
    let mut batch = original.clone();
    let block = batch["content"][0].clone();
    batch["content"].as_array_mut().unwrap().push(block);
    let mut foreign = original.clone();
    foreign["content"][0]["name"] = json!("bash");
    let mut truncated = original.clone();
    truncated["stop_reason"] = json!("max_tokens");
    let mut refusal = original.clone();
    refusal["stop_reason"] = json!("refusal");
    let mut toolset = original.clone();
    toolset["content"][0]["toolset_name"] = json!("computer");
    for value in [
        batch,
        foreign,
        truncated,
        refusal,
        toolset,
        json!({"content":[]}),
    ] {
        assert!(parse_response(&serde_json::to_vec(&value).unwrap(), &t).is_err());
    }
    assert!(parse_response(b"not JSON SECRET", &t).is_err());
    assert!(matches!(
        parse_response(&vec![0; MAX_RESPONSE + 1], &t),
        Err(ProviderError::Oversized)
    ));
}
#[tokio::test]
async fn transport_failures_do_not_retry_or_return_sensitive_error_bodies() {
    for failure in [
        ProviderError::Transport,
        ProviderError::Http(401),
        ProviderError::Http(429),
    ] {
        let (provider, calls) = adapter(Err(failure));
        let png = screenshot();
        let o = observation();
        let error = provider.propose(turn(&png, &o)).await.err().unwrap();
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert!(!format!("{error:?} {error}").contains("synthetic-key"));
    }
}
#[test]
fn key_validation_and_sensitive_header_redaction() {
    for s in ["", "has space", "secret\nheader:evil"] {
        assert!(ApiKey::from_local_secret(s).is_err());
    }
    let key = ApiKey::from_local_secret("synthetic-private-value").unwrap();
    assert!(!format!("{:?}", key.0).contains("synthetic-private-value"));
}
#[test]
fn completion_is_explicit_and_usage_is_retained() {
    let png = screenshot();
    let o = observation();
    let t = turn(&png, &o);
    let value = json!({"type":"message","role":"assistant","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2},"content":[{"type":"text","text":"Task complete"}]});
    let p = parse_response(&serde_json::to_vec(&value).unwrap(), &t).unwrap();
    assert!(
        matches!(p,Proposal::Complete{text,usage:Usage{input_tokens:1,output_tokens:2}} if text=="Task complete")
    );
}

#[test]
fn real_http_request_has_fixed_origin_beta_and_sensitive_auth() {
    let http = DirectHttp::new().unwrap();
    let key = ApiKey::from_local_secret("test-only-secret").unwrap();
    let request = http.request(&key, &json!({"test":true})).build().unwrap();
    assert_eq!(request.url().as_str(), ENDPOINT);
    assert_eq!(request.method(), reqwest::Method::POST);
    assert_eq!(request.headers()["anthropic-beta"], BETA);
    assert_eq!(request.headers()["anthropic-version"], "2023-06-01");
    assert!(request.headers()["x-api-key"].is_sensitive());
    assert!(!format!("{:?}", request.headers()).contains("test-only-secret"));
}
#[test]
fn indexed_png_transparency_survives_normalization() {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 4, 3);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(vec![255, 0, 0]);
        encoder.set_trns(vec![0]);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0; 12]).unwrap();
    }
    let o = observation();
    let body = encode_turn(Model::Opus45, &turn(&png, &o)).unwrap();
    let bytes = STANDARD
        .decode(
            body["messages"][0]["content"][1]["source"]["data"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    assert!(pixels.chunks_exact(4).all(|pixel| pixel == [255, 0, 0, 0]));
}

#[tokio::test]
async fn multimodal_budgeted_roundtrip_reaches_fake_desktop_and_spending_stops_next_call() {
    use crate::desktop::{
        limits::Limits,
        session::{Batch, DesktopProfile, DesktopSession},
        ContentBlock, Outcome,
    };
    let blocks = [
        ContentBlock::Text {
            text: "Click the button".into(),
        },
        ContentBlock::Png {
            bytes: screenshot(),
            width: 4,
            height: 3,
        },
    ];
    assert!(blocks.iter().all(ContentBlock::validate_envelope));
    let ContentBlock::Png { bytes, .. } = &blocks[1] else {
        unreachable!()
    };
    let o = observation();
    let mut t = turn(bytes, &o);
    t.sequence = 0;
    let id = t.session;
    let call = t.call_id;
    let mut limits = Limits::default();
    limits.cost_micro_usd = 10;
    let mut session = DesktopSession::new(id, &DesktopProfile::current(), limits, 0).unwrap();
    session.report_focus_change(Some(o.target.clone()), 0);
    let (provider, calls) = adapter(Ok(response(
        json!({"action":"left_click","coordinate":[1,2]}),
    )));
    let proposal = provider
        .propose_in_session(t, &mut session, 1, SpendQuote::conservative(10))
        .await
        .unwrap();
    let Proposal::Action { request, .. } = proposal else {
        panic!("action required")
    };
    let owner = Uuid::new_v4();
    let node = Uuid::new_v4();
    let a = Authority {
        session: id,
        owner,
        project_owner: owner,
        node,
        target_node: node,
        private_local: true,
        lease_deadline: 100,
        authority_deadline: 100,
        stopped: false,
        grant_deadline: 100,
        access: Access::Control,
        target: o.target.clone(),
    };
    let mut batch = Batch::new(vec![call]).unwrap();
    assert_eq!(
        session
            .execute_step(
                &mut batch,
                &a,
                &Policy::default(),
                &o,
                &request,
                Risk::Routine,
                None,
                2,
                2
            )
            .unwrap(),
        Outcome::Applied
    );
    let mut t = turn(bytes, &o);
    t.session = id;
    assert!(matches!(
        provider
            .propose_in_session(t, &mut session, 3, SpendQuote::conservative(1))
            .await,
        Err(ProviderError::Budget)
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert_eq!(session.effects(), 1);
    assert_eq!(session.counters().turns, 1);
}
#[tokio::test]
async fn failed_provider_call_keeps_reservation_without_dispatch_or_retry() {
    use crate::desktop::{
        limits::Limits,
        session::{DesktopProfile, DesktopSession},
    };
    let png = screenshot();
    let o = observation();
    let t = turn(&png, &o);
    let id = t.session;
    let mut limits = Limits::default();
    limits.turns = 1;
    let mut session = DesktopSession::new(id, &DesktopProfile::current(), limits, 0).unwrap();
    let (provider, calls) = adapter(Err(ProviderError::Transport));
    assert!(matches!(
        provider
            .propose_in_session(t, &mut session, 0, SpendQuote::conservative(10))
            .await,
        Err(ProviderError::Transport)
    ));
    assert_eq!(session.counters().cost_micro_usd, 10);
    assert_eq!(session.effects(), 0);
    let mut t = turn(&png, &o);
    t.session = id;
    assert!(matches!(
        provider
            .propose_in_session(t, &mut session, 1, SpendQuote::conservative(10))
            .await,
        Err(ProviderError::Budget)
    ));
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn trusted_pricing_settles_usage_and_overruns_stop_session() {
    use crate::desktop::{
        limits::Limits,
        session::{DesktopProfile, DesktopSession},
    };
    for ceiling in [120, 119] {
        let png = screenshot();
        let o = observation();
        let t = turn(&png, &o);
        let id = t.session;
        let mut session =
            DesktopSession::new(id, &DesktopProfile::current(), Limits::default(), 0).unwrap();
        let (provider, calls) = adapter(Ok(response(json!({"action":"screenshot"}))));
        let quote = SpendQuote {
            ceiling_micro_usd: ceiling,
            prices: Some(TokenPrices {
                input_micro_usd_per_million: 1_000_000,
                output_micro_usd_per_million: 1_000_000,
            }),
        };
        let result = provider.propose_in_session(t, &mut session, 0, quote).await;
        assert_eq!(session.counters().cost_micro_usd, 120);
        if ceiling == 120 {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(ProviderError::Budget)));
            let mut t = turn(&png, &o);
            t.session = id;
            assert!(matches!(
                provider
                    .propose_in_session(t, &mut session, 1, SpendQuote::conservative(100))
                    .await,
                Err(ProviderError::Budget)
            ));
        }
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert_eq!(session.effects(), 0);
    }
}
