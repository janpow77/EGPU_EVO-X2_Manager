//! eGPU-Agent: Registriert die EVO-X2 als Remote-GPU beim NUC-Daemon
//! und sendet periodische Heartbeats mit GTT-Metriken.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::signal;
use tracing::{error, info, warn};

#[derive(Serialize)]
struct RegisterRequest {
    name: String,
    host: String,
    port_ollama: u16,
    port_agent: u16,
    gpu_name: String,
    vram_mb: u64,
}

#[derive(Serialize)]
struct HeartbeatRequest {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_ms: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vram_mb: Option<u64>,
}

#[derive(Serialize)]
struct UnregisterRequest {
    name: String,
}

/// GTT-Total aus sysfs lesen (AMD-spezifisch).
/// Probiert /sys/class/drm/card{0,1}/device/mem_info_gtt_total
fn read_gtt_total_mb() -> Result<u64> {
    for card_id in 0..4 {
        let path = format!(
            "/sys/class/drm/card{card_id}/device/mem_info_gtt_total"
        );
        if let Ok(content) = std::fs::read_to_string(&path) {
            let bytes: u64 = content.trim().parse()
                .with_context(|| format!("Parse {path}"))?;
            let mb = bytes / (1024 * 1024);
            info!("GTT-Total: {mb} MB (aus {path})");
            return Ok(mb);
        }
    }
    bail!("Kein AMD GPU sysfs-Eintrag gefunden (mem_info_gtt_total)")
}

/// GTT-Used aus sysfs lesen.
fn read_gtt_used_mb() -> u64 {
    for card_id in 0..4 {
        let path = format!(
            "/sys/class/drm/card{card_id}/device/mem_info_gtt_used"
        );
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(bytes) = content.trim().parse::<u64>() {
                return bytes / (1024 * 1024);
            }
        }
    }
    0
}

/// Eigene Tailscale-IP ermitteln.
fn detect_local_ip() -> Result<String> {
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .context("tailscale CLI nicht gefunden")?;
    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ip.is_empty() {
        bail!("Tailscale hat keine IPv4 zurückgegeben");
    }
    info!("Lokale Tailscale-IP: {ip}");
    Ok(ip)
}

/// Token aus --token oder --token-path auflösen.
fn resolve_token(token: Option<String>, token_path: Option<String>) -> Result<String> {
    if let Some(t) = token {
        return Ok(t);
    }
    if let Some(path) = token_path {
        let t = std::fs::read_to_string(&path)
            .with_context(|| format!("Token-Datei nicht lesbar: {path}"))?;
        return Ok(t.trim().to_string());
    }
    bail!("Weder --token noch --token-path angegeben")
}

pub async fn run(
    nuc_url: &str,
    token: Option<String>,
    token_path: Option<String>,
    name: &str,
    heartbeat_interval: u64,
    port_ollama: u16,
) -> Result<()> {
    let auth_token = resolve_token(token, token_path)?;
    let local_ip = detect_local_ip()?;
    let gtt_total = read_gtt_total_mb()?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let register_url = format!("{nuc_url}/api/remote/register");
    let heartbeat_url = format!("{nuc_url}/api/remote/heartbeat");
    let unregister_url = format!("{nuc_url}/api/remote/unregister");

    // Registrierung
    let reg = RegisterRequest {
        name: name.to_string(),
        host: local_ip.clone(),
        port_ollama,
        port_agent: 8899,
        gpu_name: "AMD Radeon (Strix Halo iGPU)".to_string(),
        vram_mb: gtt_total,
    };

    info!("Registriere bei {register_url} als '{name}' ({gtt_total} MB GTT)");
    let resp = client
        .post(&register_url)
        .bearer_auth(&auth_token)
        .json(&reg)
        .send()
        .await
        .context("Register-Request fehlgeschlagen")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Registrierung fehlgeschlagen: {status} — {body}");
    }
    info!("Registrierung erfolgreich");

    // Heartbeat-Loop mit graceful shutdown
    let name_owned = name.to_string();
    let shutdown = async {
        let ctrl_c = signal::ctrl_c();
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => info!("SIGINT empfangen"),
            _ = sigterm.recv() => info!("SIGTERM empfangen"),
        }
    };

    let heartbeat_loop = async {
        let interval = std::time::Duration::from_secs(heartbeat_interval);
        loop {
            tokio::time::sleep(interval).await;

            let start = std::time::Instant::now();
            let hb = HeartbeatRequest {
                name: name_owned.clone(),
                latency_ms: None, // Wird nach Antwort berechnet
                vram_mb: Some(gtt_total.saturating_sub(read_gtt_used_mb())),
            };

            match client
                .post(&heartbeat_url)
                .bearer_auth(&auth_token)
                .json(&hb)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let rtt = start.elapsed().as_millis() as u32;
                    info!(
                        "Heartbeat OK (RTT: {rtt}ms, GTT frei: {} MB)",
                        hb.vram_mb.unwrap_or(0)
                    );
                }
                Ok(resp) => {
                    warn!("Heartbeat-Antwort: {}", resp.status());
                }
                Err(e) => {
                    error!("Heartbeat fehlgeschlagen: {e}");
                }
            }
        }
    };

    tokio::select! {
        _ = shutdown => {}
        _ = heartbeat_loop => {}
    }

    // Graceful Unregister
    info!("Deregistriere '{name_owned}'...");
    let unreg = UnregisterRequest {
        name: name_owned,
    };
    match client
        .post(&unregister_url)
        .bearer_auth(&auth_token)
        .json(&unreg)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => info!("Deregistrierung erfolgreich"),
        Ok(resp) => warn!("Deregistrierung: {}", resp.status()),
        Err(e) => warn!("Deregistrierung fehlgeschlagen: {e}"),
    }

    Ok(())
}
