//! App-owned diagnostic bridge: credentials arrive once through stdin, never argv or logs.
use github_copilot_sdk::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::{self, Read},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    token: String,
    login: String,
    model: Option<String>,
}

async fn check(
    client: &Client,
    input: &Input,
    home: &std::path::Path,
) -> Result<Value, &'static str> {
    let auth = client.get_auth_status().await.map_err(|_| "Copilot could not verify this account. Reconnect GitHub or check your Copilot subscription.")?;
    // Validate the very same explicit token here, not a caller-supplied identity
    // assertion or an unrelated gh/CLI credential. Never follow credential redirects.
    #[derive(Deserialize)]
    struct GitHubIdentity {
        login: String,
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not initialize GitHub identity verification.")?;
    let response = http
        .get("https://api.github.com/user")
        .bearer_auth(&input.token)
        .header("User-Agent", "Lokis-Den-Copilot-Check")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| {
            "Could not verify the supplied token with GitHub. No test message was sent."
        })?;
    if !response.status().is_success() {
        return Err("GitHub rejected token identity verification. Reconnect GitHub. No test message was sent.");
    }
    let verified: GitHubIdentity = response
        .json()
        .await
        .map_err(|_| "GitHub did not return a usable identity. No test message was sent.")?;
    if let Some(error) = hive_copilot_conformance::verified_token_identity_error(
        auth.is_authenticated,
        auth.login.as_deref(),
        &verified.login,
        &input.login,
    ) {
        return Err(error);
    }
    let models = client.list_models().await.map_err(|_| "Copilot model access was denied or unavailable. Check subscription and organization policy.")?;
    let ids: Vec<String> = models.iter().map(|m| m.id.clone()).collect();
    let mut reply = None;
    if let Some(model) = &input.model {
        if !ids.contains(model) {
            return Err("The selected model is no longer available. Check access again.");
        }
        let config = hive_copilot_conformance::session("hive-copilot-acceptance", model, home);
        let session = client
            .create_session(config)
            .await
            .map_err(|_| "Copilot could not start the isolated test session.")?;
        let result = session
            .send_and_wait(
                "Reply with exactly: Loki's Den Copilot connection works. Do not use any tools.",
            )
            .await;
        if result.is_err() {
            let _ = session.abort().await;
        }
        let _ = session.disconnect().await;
        let event = result.map_err(|_| "Copilot did not finish the test. It may have used subscription quota; the test will not retry automatically.")?;
        reply = event.and_then(|event| {
            event
                .data
                .get("content")
                .and_then(Value::as_str)
                .map(|s| s.chars().take(2000).collect::<String>())
        });
        if reply.is_none() {
            return Err("Copilot finished without a text response. No automatic retry was made.");
        }
    }
    Ok(json!({"login": input.login, "models": ids, "reply": reply}))
}

#[tokio::main]
async fn main() {
    let mut raw = Vec::new();
    if io::stdin().take(32769).read_to_end(&mut raw).is_err() || raw.len() > 32768 {
        println!("{}", json!({"error":"Invalid account request."}));
        return;
    }
    let Ok(input) = serde_json::from_slice::<Input>(&raw) else {
        println!("{}", json!({"error":"Invalid account request."}));
        return;
    };
    let home = std::env::temp_dir().join(format!("hive-copilot-check-{}", std::process::id()));
    if std::fs::create_dir(&home).is_err() {
        println!(
            "{}",
            json!({"error":"Could not create a fresh Copilot test directory."})
        );
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700)).is_err() {
            return;
        }
    }
    let result = async {
        let options = hive_copilot_conformance::options(input.token.clone(), &home)?;
        let client = Client::start(options)
            .await
            .map_err(|_| "Copilot runtime could not start.")?;
        let checked =
            tokio::time::timeout(Duration::from_secs(85), check(&client, &input, &home)).await;
        let _ = tokio::time::timeout(Duration::from_secs(5), client.stop()).await;
        checked.map_err(|_| "Copilot check timed out. No automatic retry was made.")?
    };
    let output = match tokio::time::timeout(Duration::from_secs(110), result).await {
        Ok(Ok(value)) => value,
        Ok(Err(message)) => json!({"error":message}),
        Err(_) => json!({"error":"Copilot check timed out. No automatic retry was made."}),
    };
    let _ = std::fs::remove_dir_all(home);
    println!("{output}");
}
