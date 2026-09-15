//! ADR-033 Stage 1's gate, verbatim from section 10: "interleaved requests/events, bounded
//! buffers, crash/restart and unknown-message tests pass." One test per clause, each driving a
//! real `Supervisor` against a `FakeServer` over an in-memory `tokio::io::duplex` pair -- no real
//! `codex` binary, no network, no credentials.

use super::*;

fn pipe() -> (
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
) {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (client_read, client_write) = tokio::io::split(client_io);
    let (server_read, server_write) = tokio::io::split(server_io);
    (client_read, client_write, server_read, server_write)
}

#[tokio::test]
async fn interleaved_requests_and_events_correlate_correctly() {
    let (client_read, client_write, server_read, server_write) = pipe();
    let mut sup = Supervisor::new(client_read, client_write, FrameLimits::default(), 1);
    let mut fake = FakeServer::new(server_read, server_write, FrameLimits::default().max_frame_bytes);

    // Two calls go out before either is answered.
    let id_a = sup.call("model/list", None).await.unwrap();
    let id_b = sup
        .call("account/read", Some(serde_json::json!({"refreshToken": false})))
        .await
        .unwrap();
    assert_ne!(id_a, id_b);
    assert!(sup.is_in_flight(id_a));
    assert!(sup.is_in_flight(id_b));

    let req1 = fake.next_request_id().await.unwrap().unwrap();
    let req2 = fake.next_request_id().await.unwrap().unwrap();
    assert_eq!(req1, id_a);
    assert_eq!(req2, id_b);

    // Server answers out of order and slips a notification in between the two responses.
    fake.reply_ok(id_b, serde_json::json!({"account": "signed_out"}))
        .await
        .unwrap();
    fake.push_notification("account/updated", Some(serde_json::json!({"state": "ready"})))
        .await
        .unwrap();
    fake.reply_ok(id_a, serde_json::json!({"models": []})).await.unwrap();

    let first = sup.pump_once().await.unwrap().unwrap();
    assert!(first.is_empty());
    assert_eq!(
        sup.take_result(id_b).unwrap().unwrap().unwrap(),
        serde_json::json!({"account": "signed_out"})
    );
    assert!(!sup.is_in_flight(id_b));
    assert!(sup.is_in_flight(id_a));

    let second = sup.pump_once().await.unwrap().unwrap();
    assert_eq!(
        second,
        vec![CoordinatorEvent::AuthChanged(serde_json::json!({"state": "ready"}))]
    );
    assert_eq!(sup.auth_state(), AuthState::Ready);

    let third = sup.pump_once().await.unwrap().unwrap();
    assert!(third.is_empty());
    assert_eq!(
        sup.take_result(id_a).unwrap().unwrap().unwrap(),
        serde_json::json!({"models": []})
    );
    assert!(!sup.is_in_flight(id_a));
}

#[tokio::test]
async fn outstanding_call_budget_is_enforced() {
    let (client_read, client_write, server_read, server_write) = pipe();
    let mut limits = FrameLimits::default();
    limits.max_queued_control = 2;
    let mut sup = Supervisor::new(client_read, client_write, limits, 1);
    let mut fake = FakeServer::new(server_read, server_write, limits.max_frame_bytes);

    let id1 = sup.call("model/list", None).await.unwrap();
    let _id2 = sup.call("model/list", None).await.unwrap();
    let err = sup.call("model/list", None).await.unwrap_err();
    assert!(matches!(err, TransportError::QueueFull));

    let req = fake.next_request_id().await.unwrap().unwrap();
    assert_eq!(req, id1);
    fake.reply_ok(id1, serde_json::json!(null)).await.unwrap();
    sup.pump_once().await.unwrap();
    assert!(!sup.is_in_flight(id1));

    // Draining one response frees a slot for a new call.
    let id3 = sup.call("model/list", None).await.unwrap();
    assert!(sup.is_in_flight(id3));
}

#[tokio::test]
async fn reconnect_abandons_stale_in_flight_calls_and_bumps_generation() {
    let (client_read_a, client_write_a, server_read_a, _server_write_a) = pipe();
    let mut sup = Supervisor::new(client_read_a, client_write_a, FrameLimits::default(), 1);
    assert_eq!(sup.generation(), 1);

    let id1 = sup.call("model/list", None).await.unwrap();
    assert!(sup.is_in_flight(id1));

    // Server A "crashes" -- its read half is dropped, so nothing further will ever arrive on
    // this connection. The client never observes this directly; `reconnect` doesn't need it to.
    drop(server_read_a);

    let (client_read_b, client_write_b, server_read_b, server_write_b) = pipe();
    let mut fake_b = FakeServer::new(server_read_b, server_write_b, FrameLimits::default().max_frame_bytes);

    sup.reconnect(client_read_b, client_write_b);
    assert_eq!(sup.generation(), 2);
    assert!(!sup.is_in_flight(id1), "a call from before reconnect must be abandoned, not resolvable");

    // A fresh call on the new connection behaves normally.
    let id2 = sup.call("model/list", None).await.unwrap();
    let req = fake_b.next_request_id().await.unwrap().unwrap();
    assert_eq!(req, id2);
    fake_b.reply_ok(id2, serde_json::json!({"ok": true})).await.unwrap();
    sup.pump_once().await.unwrap();
    assert_eq!(
        sup.take_result(id2).unwrap().unwrap().unwrap(),
        serde_json::json!({"ok": true})
    );
}

#[tokio::test]
async fn unknown_notification_and_server_request_are_handled_not_dropped_or_hung() {
    let (client_read, client_write, server_read, server_write) = pipe();
    let mut sup = Supervisor::new(client_read, client_write, FrameLimits::default(), 1);
    let mut fake = FakeServer::new(server_read, server_write, FrameLimits::default().max_frame_bytes);

    fake.push_notification("thread/experimental/weirdEvent", Some(serde_json::json!({"x": 1})))
        .await
        .unwrap();
    let events = sup.pump_once().await.unwrap().unwrap();
    assert_eq!(
        events,
        vec![CoordinatorEvent::Diagnostic {
            method: "thread/experimental/weirdEvent".to_string(),
            params: Some(serde_json::json!({"x": 1})),
        }]
    );

    fake.push_server_request(77, "approval/somethingNew", Some(serde_json::json!({"why": "test"})))
        .await
        .unwrap();
    let events2 = sup.pump_once().await.unwrap().unwrap();
    assert_eq!(
        events2,
        vec![CoordinatorEvent::Diagnostic {
            method: "approval/somethingNew".to_string(),
            params: Some(serde_json::json!({"why": "test"})),
        }]
    );

    // The client must have written an auto-decline back, not left the server request hanging.
    let raw = fake.read_raw_frame().await.unwrap().unwrap();
    let decline: RawResponse = serde_json::from_slice(&raw).unwrap();
    assert_eq!(decline.id, 77);
    assert!(decline.result.is_none());
    let error = decline.error.unwrap();
    assert_eq!(error.code, -32601);
    assert!(error.message.contains("approval/somethingNew"));
}
