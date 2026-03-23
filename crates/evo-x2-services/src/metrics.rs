use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tracing::debug;

const SERVICES: &[&str] = &["llama-creative", "llama-code", "llama-embedding", "llama-ocr"];
const CACHE_TTL_SECS: u64 = 3;

#[derive(Clone)]
struct AppState {
    svc_cache: std::sync::Arc<Mutex<SvcCache>>,
}

struct SvcCache {
    data: HashMap<String, String>,
    last_update: Instant,
}

#[derive(Serialize)]
struct MetricsResponse {
    gtt: GttInfo,
    ram: MemInfo,
    cpu_load: CpuLoad,
    services: HashMap<String, String>,
}

#[derive(Serialize)]
struct GttInfo {
    used_bytes: u64,
    total_bytes: u64,
    used_gb: f64,
    total_gb: f64,
}

#[derive(Serialize)]
struct MemInfo {
    used_bytes: u64,
    total_bytes: u64,
    used_gb: f64,
    total_gb: f64,
}

#[derive(Serialize)]
struct CpuLoad {
    #[serde(rename = "1min")]
    min1: f64,
    #[serde(rename = "5min")]
    min5: f64,
    #[serde(rename = "15min")]
    min15: f64,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    let state = AppState {
        svc_cache: std::sync::Arc::new(Mutex::new(SvcCache {
            data: HashMap::new(),
            last_update: Instant::now() - std::time::Duration::from_secs(CACHE_TTL_SECS + 1),
        })),
    };

    let app = Router::new()
        .route("/metrics", get(handle_metrics))
        .route("/health", get(handle_health))
        .with_state(state);

    let addr: std::net::SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn handle_metrics(State(state): State<AppState>) -> Json<MetricsResponse> {
    let gtt = read_gtt();
    let ram = read_memory();
    let cpu = read_cpu_load();
    let services = read_services_cached(&state);

    Json(MetricsResponse {
        gtt,
        ram,
        cpu_load: cpu,
        services,
    })
}

fn read_gtt() -> GttInfo {
    let used = read_sysfs_u64("/sys/class/drm/card0/device/mem_info_gtt_used")
        .or_else(|| read_sysfs_u64("/sys/class/drm/card1/device/mem_info_gtt_used"))
        .unwrap_or(0);
    let total = read_sysfs_u64("/sys/class/drm/card0/device/mem_info_gtt_total")
        .or_else(|| read_sysfs_u64("/sys/class/drm/card1/device/mem_info_gtt_total"))
        .unwrap_or(0);

    GttInfo {
        used_bytes: used,
        total_bytes: total,
        used_gb: used as f64 / 1024.0_f64.powi(3),
        total_gb: total as f64 / 1024.0_f64.powi(3),
    }
}

fn read_memory() -> MemInfo {
    let mut total: u64 = 0;
    let mut available: u64 = 0;

    if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total = parse_meminfo_kb(line) * 1024;
            } else if line.starts_with("MemAvailable:") {
                available = parse_meminfo_kb(line) * 1024;
            }
        }
    }

    let used = total.saturating_sub(available);
    MemInfo {
        used_bytes: used,
        total_bytes: total,
        used_gb: used as f64 / 1024.0_f64.powi(3),
        total_gb: total as f64 / 1024.0_f64.powi(3),
    }
}

fn read_cpu_load() -> CpuLoad {
    if let Ok(content) = std::fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = content.split_whitespace().collect();
        if parts.len() >= 3 {
            return CpuLoad {
                min1: parts[0].parse().unwrap_or(0.0),
                min5: parts[1].parse().unwrap_or(0.0),
                min15: parts[2].parse().unwrap_or(0.0),
            };
        }
    }
    CpuLoad { min1: 0.0, min5: 0.0, min15: 0.0 }
}

fn read_services_cached(state: &AppState) -> HashMap<String, String> {
    let mut cache = state.svc_cache.lock().unwrap();
    if cache.last_update.elapsed().as_secs() < CACHE_TTL_SECS && !cache.data.is_empty() {
        debug!("Service-Status aus Cache");
        return cache.data.clone();
    }

    let mut status = HashMap::new();
    for svc in SERVICES {
        let result = std::process::Command::new("systemctl")
            .args(["is-active", svc])
            .output();
        let s = match result {
            Ok(output) => String::from_utf8_lossy(&output.stdout).trim().to_string(),
            Err(_) => "unknown".into(),
        };
        status.insert(svc.to_string(), s);
    }

    cache.data = status.clone();
    cache.last_update = Instant::now();
    status
}

fn read_sysfs_u64(path: &str) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn parse_meminfo_kb(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
