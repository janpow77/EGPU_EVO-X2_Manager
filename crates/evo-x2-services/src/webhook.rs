use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::{info, warn};

const MAX_PAYLOAD: usize = 10 * 1024 * 1024; // 10 MB
const UPDATE_SCRIPT: &str = ".local/bin/llama-update.sh";
const INVALID_SECRETS: &[&str] = &["", "NICHT_GESETZT", "your-github-webhook-secret"];

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
struct AppState {
    secret: String,
    update_script: String,
}

#[derive(Serialize)]
struct WebhookResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#ref: Option<String>,
}

#[derive(Deserialize)]
struct PushEvent {
    #[serde(default)]
    r#ref: String,
}

pub async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    let secret = load_secret()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jan".into());
    let update_script = format!("{home}/{UPDATE_SCRIPT}");

    // Pruefe ob Update-Script existiert
    if !std::path::Path::new(&update_script).exists() {
        warn!("Update-Script nicht gefunden: {update_script}");
    }

    let state = AppState { secret, update_script };

    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/webhook", post(handle_webhook))
        .with_state(state);

    let addr: std::net::SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_health() -> Json<WebhookResponse> {
    Json(WebhookResponse {
        status: "ok".into(),
        error: None,
        r#ref: None,
    })
}

async fn handle_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<WebhookResponse>) {
    // Payload-Limit
    if body.len() > MAX_PAYLOAD {
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(WebhookResponse {
            status: "error".into(),
            error: Some("payload too large".into()),
            r#ref: None,
        }));
    }

    // HMAC-Validierung
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !verify_signature(&body, signature, &state.secret) {
        warn!("Ungueltige Signatur!");
        return (StatusCode::FORBIDDEN, Json(WebhookResponse {
            status: "error".into(),
            error: Some("invalid signature".into()),
            r#ref: None,
        }));
    }

    // JSON parsen
    let event: PushEvent = match serde_json::from_slice(&body) {
        Ok(e) => e,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(WebhookResponse {
            status: "error".into(),
            error: Some("invalid json".into()),
            r#ref: None,
        })),
    };

    // Nur Push auf main/master
    if event.r#ref != "refs/heads/main" && event.r#ref != "refs/heads/master" {
        info!("Ignoriere Push auf {}", event.r#ref);
        return (StatusCode::OK, Json(WebhookResponse {
            status: "ignored".into(),
            error: None,
            r#ref: Some(event.r#ref),
        }));
    }

    info!("Push auf {} — starte Update...", event.r#ref);

    // Update im Hintergrund starten
    let script = state.update_script.clone();
    tokio::spawn(async move {
        match tokio::process::Command::new(&script)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => info!("Update-Script gestartet"),
            Err(e) => warn!("Update-Script fehlgeschlagen: {e}"),
        }
    });

    (StatusCode::OK, Json(WebhookResponse {
        status: "update triggered".into(),
        error: None,
        r#ref: Some(event.r#ref),
    }))
}

fn verify_signature(payload: &[u8], signature: &str, secret: &str) -> bool {
    let Some(hex_sig) = signature.strip_prefix("sha256=") else {
        return false;
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };

    mac.update(payload);
    let expected = hex::encode(mac.finalize().into_bytes());

    // Constant-time comparison
    subtle::ConstantTimeEq::ct_eq(expected.as_bytes(), hex_sig.as_bytes()).into()
}

fn load_secret() -> anyhow::Result<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/jan".into());
    let env_path = format!("{home}/.config/evo-x2/.env");

    let content = std::fs::read_to_string(&env_path)
        .map_err(|e| anyhow::anyhow!("{env_path} nicht lesbar: {e}"))?;

    for line in content.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("WEBHOOK_SECRET=") {
            let secret = value.trim().trim_matches('"').trim_matches('\'').to_string();
            if INVALID_SECRETS.contains(&secret.as_str()) {
                anyhow::bail!("WEBHOOK_SECRET ist nicht konfiguriert ({secret})");
            }
            return Ok(secret);
        }
    }

    anyhow::bail!("WEBHOOK_SECRET nicht in {env_path} gefunden")
}
