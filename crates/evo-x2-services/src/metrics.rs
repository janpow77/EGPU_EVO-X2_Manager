use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::debug;

const SERVICES: &[&str] = &["ollama", "evo-metrics", "evo-webhook"];
const SVC_CACHE_TTL: u64 = 3;
const OLLAMA_CACHE_TTL: u64 = 5;
const SYSTEM_CACHE_TTL: u64 = 300;
const TAILSCALE_CACHE_TTL: u64 = 60;
const DISK_CACHE_TTL: u64 = 30;

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    svc_cache: std::sync::Arc<Mutex<TimedCache<HashMap<String, String>>>>,
    ollama_cache: std::sync::Arc<Mutex<TimedCache<OllamaInfo>>>,
    system_cache: std::sync::Arc<Mutex<TimedCache<SystemInfo>>>,
    tailscale_cache: std::sync::Arc<Mutex<TimedCache<TailscaleInfo>>>,
    disk_cache: std::sync::Arc<Mutex<TimedCache<Vec<DiskInfo>>>>,
    http: reqwest::Client,
}

struct TimedCache<T> {
    data: Option<T>,
    last_update: Instant,
    ttl_secs: u64,
}

impl<T: Clone> TimedCache<T> {
    fn new(ttl_secs: u64) -> Self {
        Self {
            data: None,
            last_update: Instant::now() - Duration::from_secs(ttl_secs + 1),
            ttl_secs,
        }
    }

    fn get(&self) -> Option<T> {
        if self.last_update.elapsed().as_secs() < self.ttl_secs {
            self.data.clone()
        } else {
            None
        }
    }

    fn set(&mut self, value: T) {
        self.data = Some(value);
        self.last_update = Instant::now();
    }
}

// ---------------------------------------------------------------------------
// Response-Typen
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct MetricsResponse {
    gtt: GttInfo,
    ram: MemInfo,
    cpu_load: CpuLoad,
    services: HashMap<String, String>,
    gpu: GpuInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    ollama: Option<OllamaInfo>,
    disks: Vec<DiskInfo>,
    system: SystemInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    tailscale: Option<TailscaleInfo>,
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

#[derive(Serialize, Clone)]
struct GpuInfo {
    temperature_c: Option<u32>,
    utilization_pct: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
struct OllamaInfo {
    running_models: Vec<OllamaModel>,
    available_models: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct OllamaModel {
    name: String,
    size_gb: f64,
    vram_gb: f64,
    processor: String,
    expires_at: Option<String>,
}

#[derive(Serialize, Clone)]
struct DiskInfo {
    mount: String,
    total_gb: f64,
    used_gb: f64,
    available_gb: f64,
}

#[derive(Serialize, Clone)]
struct SystemInfo {
    soc: String,
    gpu_arch: String,
    ram_spec: String,
    cpu_cores: u32,
    uptime_seconds: u64,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self {
            soc: String::new(),
            gpu_arch: String::new(),
            ram_spec: String::new(),
            cpu_cores: 0,
            uptime_seconds: 0,
        }
    }
}

#[derive(Serialize, Clone)]
struct TailscaleInfo {
    ip: Option<String>,
    hostname: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

pub async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    let state = AppState {
        svc_cache: std::sync::Arc::new(Mutex::new(TimedCache::new(SVC_CACHE_TTL))),
        ollama_cache: std::sync::Arc::new(Mutex::new(TimedCache::new(OLLAMA_CACHE_TTL))),
        system_cache: std::sync::Arc::new(Mutex::new(TimedCache::new(SYSTEM_CACHE_TTL))),
        tailscale_cache: std::sync::Arc::new(Mutex::new(TimedCache::new(TAILSCALE_CACHE_TTL))),
        disk_cache: std::sync::Arc::new(Mutex::new(TimedCache::new(DISK_CACHE_TTL))),
        http,
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
    let gpu = read_gpu_info();
    let services = read_services_cached(&state.svc_cache);
    let ollama = read_ollama_cached(&state).await;
    let disks = read_disks_cached(&state.disk_cache);
    let system = read_system_cached(&state.system_cache);
    let tailscale = read_tailscale_cached(&state.tailscale_cache);

    Json(MetricsResponse {
        gtt,
        ram,
        cpu_load: cpu,
        services,
        gpu,
        ollama,
        disks,
        system,
        tailscale,
    })
}

// ---------------------------------------------------------------------------
// GTT (AMD GPU-Speicher)
// ---------------------------------------------------------------------------

fn read_gtt() -> GttInfo {
    let (used, total) = find_gtt_sysfs();
    GttInfo {
        used_bytes: used,
        total_bytes: total,
        used_gb: used as f64 / 1024.0_f64.powi(3),
        total_gb: total as f64 / 1024.0_f64.powi(3),
    }
}

fn find_gtt_sysfs() -> (u64, u64) {
    let drm_dir = std::path::Path::new("/sys/class/drm");
    if let Ok(entries) = std::fs::read_dir(drm_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let total_path = path.join("device/mem_info_gtt_total");
            if total_path.exists() {
                let total = read_sysfs_u64(total_path.to_str().unwrap_or_default()).unwrap_or(0);
                if total > 0 {
                    let used_path = path.join("device/mem_info_gtt_used");
                    let used = read_sysfs_u64(used_path.to_str().unwrap_or_default()).unwrap_or(0);
                    return (used, total);
                }
            }
        }
    }
    (0, 0)
}

// ---------------------------------------------------------------------------
// GPU Info (Temperatur + Auslastung)
// ---------------------------------------------------------------------------

fn read_gpu_info() -> GpuInfo {
    let drm_dir = std::path::Path::new("/sys/class/drm");
    let mut temp = None;
    let mut util = None;

    if let Ok(entries) = std::fs::read_dir(drm_dir) {
        for entry in entries.flatten() {
            let dev = entry.path().join("device");
            // GPU-Auslastung
            if util.is_none() {
                if let Some(v) = read_sysfs_u64(dev.join("gpu_busy_percent").to_str().unwrap_or_default()) {
                    util = Some(v as u32);
                }
            }
            // Temperatur via hwmon
            if temp.is_none() {
                if let Ok(hwmons) = std::fs::read_dir(dev.join("hwmon")) {
                    for hw in hwmons.flatten() {
                        let temp_path = hw.path().join("temp1_input");
                        if let Some(millideg) = read_sysfs_u64(temp_path.to_str().unwrap_or_default()) {
                            temp = Some((millideg / 1000) as u32);
                            break;
                        }
                    }
                }
            }
            if temp.is_some() && util.is_some() {
                break;
            }
        }
    }

    GpuInfo {
        temperature_c: temp,
        utilization_pct: util,
    }
}

// ---------------------------------------------------------------------------
// RAM + CPU
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Services (systemctl is-active)
// ---------------------------------------------------------------------------

fn read_services_cached(cache: &std::sync::Arc<Mutex<TimedCache<HashMap<String, String>>>>) -> HashMap<String, String> {
    let mut c = cache.lock().unwrap();
    if let Some(cached) = c.get() {
        debug!("Service-Status aus Cache");
        return cached;
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

    c.set(status.clone());
    status
}

// ---------------------------------------------------------------------------
// Ollama (laufende + verfügbare Modelle)
// ---------------------------------------------------------------------------

async fn read_ollama_cached(state: &AppState) -> Option<OllamaInfo> {
    {
        let c = state.ollama_cache.lock().unwrap();
        if let Some(cached) = c.get() {
            return Some(cached);
        }
    }

    let info = read_ollama(&state.http).await;
    if let Some(ref i) = info {
        let mut c = state.ollama_cache.lock().unwrap();
        c.set(i.clone());
    }
    info
}

async fn read_ollama(client: &reqwest::Client) -> Option<OllamaInfo> {
    // Laufende Modelle
    let running = match client.get("http://localhost:11434/api/ps").send().await {
        Ok(resp) => {
            let json: serde_json::Value = resp.json().await.ok()?;
            let models = json.get("models")?.as_array()?;
            models.iter().filter_map(|m| {
                Some(OllamaModel {
                    name: m.get("name")?.as_str()?.to_string(),
                    size_gb: m.get("size")?.as_u64()? as f64 / 1024.0_f64.powi(3),
                    vram_gb: m.get("size_vram")?.as_u64().unwrap_or(0) as f64 / 1024.0_f64.powi(3),
                    processor: m.get("details")
                        .and_then(|d| d.get("processor"))
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string(),
                    expires_at: m.get("expires_at").and_then(|v| v.as_str()).map(|s| s.to_string()),
                })
            }).collect()
        }
        Err(e) => {
            debug!("Ollama /api/ps nicht erreichbar: {e}");
            return None;
        }
    };

    // Verfügbare Modelle
    let available = match client.get("http://localhost:11434/api/tags").send().await {
        Ok(resp) => {
            let json: serde_json::Value = resp.json().await.unwrap_or_default();
            json.get("models")
                .and_then(|m| m.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        }
        Err(_) => vec![],
    };

    Some(OllamaInfo {
        running_models: running,
        available_models: available,
    })
}

// ---------------------------------------------------------------------------
// Disk
// ---------------------------------------------------------------------------

fn read_disks_cached(cache: &std::sync::Arc<Mutex<TimedCache<Vec<DiskInfo>>>>) -> Vec<DiskInfo> {
    let mut c = cache.lock().unwrap();
    if let Some(cached) = c.get() {
        return cached;
    }

    let disks = read_disks();
    c.set(disks.clone());
    disks
}

fn read_disks() -> Vec<DiskInfo> {
    let output = std::process::Command::new("df")
        .args(["-B1", "--output=target,size,used,avail", "/"])
        .output();

    let mut disks = Vec::new();
    if let Ok(out) = output {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let total: u64 = parts[1].parse().unwrap_or(0);
                let used: u64 = parts[2].parse().unwrap_or(0);
                let avail: u64 = parts[3].parse().unwrap_or(0);
                if total > 0 {
                    disks.push(DiskInfo {
                        mount: parts[0].to_string(),
                        total_gb: total as f64 / 1024.0_f64.powi(3),
                        used_gb: used as f64 / 1024.0_f64.powi(3),
                        available_gb: avail as f64 / 1024.0_f64.powi(3),
                    });
                }
            }
        }
    }
    disks
}

// ---------------------------------------------------------------------------
// System-Info
// ---------------------------------------------------------------------------

fn read_system_cached(cache: &std::sync::Arc<Mutex<TimedCache<SystemInfo>>>) -> SystemInfo {
    let mut c = cache.lock().unwrap();
    if let Some(cached) = c.get() {
        // Aktualisiere nur Uptime (ändert sich ständig)
        let mut info = cached;
        info.uptime_seconds = read_uptime();
        return info;
    }

    let info = read_system_info();
    c.set(info.clone());
    info
}

fn read_system_info() -> SystemInfo {
    let soc = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|content| {
            content.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default();

    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(0);

    let ram_total_gb = {
        let bytes = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|c| {
                c.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .map(|l| parse_meminfo_kb(l) * 1024)
            })
            .unwrap_or(0);
        (bytes as f64 / 1024.0_f64.powi(3)).round() as u64
    };

    // GPU-Architektur aus DRM-Treiber-Info
    let gpu_arch = find_gpu_arch().unwrap_or_default();

    SystemInfo {
        soc,
        gpu_arch,
        ram_spec: format!("{ram_total_gb} GB"),
        cpu_cores,
        uptime_seconds: read_uptime(),
    }
}

fn read_uptime() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|c| c.split_whitespace().next()?.parse::<f64>().ok())
        .map(|s| s as u64)
        .unwrap_or(0)
}

fn find_gpu_arch() -> Option<String> {
    let drm_dir = std::path::Path::new("/sys/class/drm");
    if let Ok(entries) = std::fs::read_dir(drm_dir) {
        for entry in entries.flatten() {
            let dev = entry.path().join("device");
            // Prüfe ob es eine echte GPU ist (hat mem_info_gtt_total)
            if dev.join("mem_info_gtt_total").exists() {
                // Versuche product_name
                if let Ok(name) = std::fs::read_to_string(dev.join("product_name")) {
                    return Some(name.trim().to_string());
                }
                // Fallback: Vendor + Device aus uevent
                if let Ok(uevent) = std::fs::read_to_string(dev.join("uevent")) {
                    let driver = uevent.lines()
                        .find(|l| l.starts_with("DRIVER="))
                        .map(|l| l.trim_start_matches("DRIVER="))
                        .unwrap_or("");
                    if driver == "amdgpu" {
                        return Some("RDNA 3.5 (amdgpu)".to_string());
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tailscale
// ---------------------------------------------------------------------------

fn read_tailscale_cached(cache: &std::sync::Arc<Mutex<TimedCache<TailscaleInfo>>>) -> Option<TailscaleInfo> {
    let mut c = cache.lock().unwrap();
    if let Some(cached) = c.get() {
        return Some(cached);
    }

    let info = read_tailscale()?;
    c.set(info.clone());
    Some(info)
}

fn read_tailscale() -> Option<TailscaleInfo> {
    let ip = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let hostname = std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if ip.is_none() {
        return None;
    }

    Some(TailscaleInfo { ip, hostname })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
