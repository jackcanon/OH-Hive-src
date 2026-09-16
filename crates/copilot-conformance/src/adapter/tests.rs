use super::*;
use hive_core::subscription::{
    journal::Journal,
    results::DurableResults,
    runner::{ResultStore, TurnRunner},
};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("hive-sdk-adapter-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(path)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn binding() -> Binding {
    Binding {
        session: Uuid::new_v4(),
        owner: Uuid::new_v4(),
        host: Uuid::new_v4(),
        agent: Uuid::new_v4(),
        conversation: Uuid::new_v4(),
        account: Uuid::new_v4(),
        provider: Provider::Copilot,
        workspace: Uuid::new_v4(),
        policy_revision: "1".into(),
    }
}
async fn read(stream: &mut DuplexStream) -> Value {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        header.push(stream.read_u8().await.unwrap());
        assert!(header.len() < 100);
    }
    let len = std::str::from_utf8(&header)
        .unwrap()
        .trim()
        .strip_prefix("Content-Length: ")
        .unwrap()
        .parse::<usize>()
        .unwrap();
    let mut body = vec![0; len];
    stream.read_exact(&mut body).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}
async fn write(stream: &mut DuplexStream, value: Value) {
    let body = serde_json::to_vec(&value).unwrap();
    stream
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await
        .unwrap();
    stream.write_all(&body).await.unwrap();
    stream.flush().await.unwrap();
}
fn fake(
    binding: &Binding,
    home: &Path,
    fail_send: bool,
) -> (
    CopilotRuntime,
    Arc<Mutex<Vec<String>>>,
    tokio::task::JoinHandle<()>,
) {
    let (client_write, mut server_read) = tokio::io::duplex(32768);
    let (mut server_write, client_read) = tokio::io::duplex(32768);
    let client = Client::from_streams(client_read, client_write, home.into()).unwrap();
    let methods = Arc::new(Mutex::new(Vec::new()));
    let seen = methods.clone();
    let task = tokio::spawn(async move {
        let mut session = String::new();
        loop {
            let request = read(&mut server_read).await;
            let method = request["method"].as_str().unwrap();
            seen.lock().await.push(method.into());
            let result = match method {
                "models.list" => {
                    json!({"models":[{"id":"auto","name":"Automatic","capabilities":{}}]})
                }
                "session.create" => {
                    session = request["params"]["sessionId"].as_str().unwrap().into();
                    assert_eq!(request["params"]["availableTools"], json!([]));
                    assert_eq!(request["params"]["requestPermission"], true);
                    json!({"sessionId":session})
                }
                "session.send" => {
                    assert_eq!(request["params"]["prompt"], "Hello from the room");
                    json!({"messageId":"user-1"})
                }
                "session.detach" | "session.abort" => json!({}),
                other => panic!("Unexpected RPC: {other}"),
            };
            if method == "session.send" && fail_send {
                write(&mut server_write,json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"uncertain delivery"}})).await;
                continue;
            }
            write(
                &mut server_write,
                json!({"jsonrpc":"2.0","id":request["id"],"result":result}),
            )
            .await;
            if method == "session.send" {
                for (kind, data) in [
                    (
                        "assistant.message",
                        json!({"content":"A saved Copilot reply"}),
                    ),
                    ("session.idle", json!({})),
                ] {
                    write(&mut server_write,json!({"jsonrpc":"2.0","method":"session.event","params":{"sessionId":session,"event":{"id":Uuid::new_v4().to_string(),"timestamp":"2026-09-16T00:00:00Z","type":kind,"data":data}}})).await;
                }
            }
        }
    });
    (
        CopilotRuntime {
            client,
            binding: binding.clone(),
            home: home.into(),
            active: Mutex::new(None),
            sending: Mutex::new(()),
        },
        methods,
        task,
    )
}
#[tokio::test]
async fn sdk_reply_persists_and_replay_does_not_send() {
    let home = Home::new();
    let binding = binding();
    let operation = Uuid::new_v4();
    let path = home.0.join("turns.db");
    let mut journal = Journal::open(&path).unwrap();
    let results = DurableResults::open(&path).unwrap();
    let (runtime, methods, server) = fake(&binding, &home.0, false);
    let runner = TurnRunner { runtime, results };
    let (_tx, cancel) = tokio::sync::watch::channel(false);
    let envelope = Envelope {
        model: "auto".into(),
        prompt: "Hello from the room".into(),
    };
    let done = runner
        .run(
            &mut journal,
            &binding,
            operation,
            &envelope,
            true,
            cancel.clone(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    drop(journal);
    let mut journal = Journal::open(&path).unwrap();
    let replay = runner
        .run(
            &mut journal,
            &binding,
            operation,
            &envelope,
            true,
            cancel,
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(done.receipt, replay.receipt);
    assert_eq!(
        runner
            .results
            .load(&binding, operation)
            .await
            .unwrap()
            .unwrap()
            .text,
        "A saved Copilot reply"
    );
    assert_eq!(
        methods
            .lock()
            .await
            .iter()
            .filter(|m| *m == "session.send")
            .count(),
        1
    );
    assert!(!server.is_finished());
    server.abort();
}
#[tokio::test]
async fn uncertain_sdk_send_aborts_but_never_resends() {
    let home = Home::new();
    let binding = binding();
    let operation = Uuid::new_v4();
    let path = home.0.join("turns.db");
    let mut journal = Journal::open(&path).unwrap();
    let (runtime, methods, server) = fake(&binding, &home.0, true);
    let runner = TurnRunner {
        runtime,
        results: DurableResults::open(&path).unwrap(),
    };
    let (_tx, cancel) = tokio::sync::watch::channel(false);
    let envelope = Envelope {
        model: "auto".into(),
        prompt: "Hello from the room".into(),
    };
    for _ in 0..2 {
        assert!(runner
            .run(
                &mut journal,
                &binding,
                operation,
                &envelope,
                true,
                cancel.clone(),
                Duration::from_secs(5)
            )
            .await
            .is_err());
    }
    assert!(matches!(
        runner
            .reconcile(&mut journal, &binding, operation, true, cancel)
            .await,
        Err(RunError::Unknown)
    ));
    let seen = methods.lock().await;
    assert_eq!(seen.iter().filter(|m| *m == "session.send").count(), 1);
    assert!(seen.iter().any(|m| m == "session.abort"));
    assert!(!server.is_finished());
    server.abort();
}
#[test]
fn runtime_home_cannot_be_reused_for_another_account() {
    let home = Home::new();
    let mut binding = binding();
    bind_home(&home.0, &binding).unwrap();
    bind_home(&home.0, &binding).unwrap();
    binding.account = Uuid::new_v4();
    assert!(bind_home(&home.0, &binding).is_err());
    assert!(bind_home(Path::new("relative"), &binding).is_err());
}
#[tokio::test]
async fn no_cloud_consent_starts_nothing() {
    let home = Home::new();
    assert!(matches!(
        CopilotRuntime::start(binding(), "not-a-token".into(), "owner", &home.0, false).await,
        Err(RunError::CloudDisabled)
    ));
    assert!(!home.0.join("hive-binding.json").exists());
}
