use serde::{Deserialize, Serialize};

// ── ubus JSON-RPC ──

#[derive(Serialize)]
pub struct UbusRequest {
    pub jsonrpc: &'static str,
    pub id: u32,
    pub method: &'static str,
    pub params: serde_json::Value,
}

impl UbusRequest {
    pub fn call(session: &str, service: &str, method: &str, args: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id: 1,
            method: "call",
            params: serde_json::json!([session, service, method, args]),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct UbusResponse {
    pub jsonrpc: String,
    pub id: u32,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

impl UbusResponse {
    /// Extract the data object from ubus result.
    /// ubus returns [status_code, {data}] on success.
    pub fn data(&self) -> Option<&serde_json::Value> {
        self.result.as_ref().and_then(|r| r.as_array()).and_then(|arr| {
            if arr.len() >= 2 && arr[0].as_i64() == Some(0) {
                Some(&arr[1])
            } else {
                None
            }
        })
    }

    pub fn is_ok(&self) -> bool {
        self.data().is_some()
    }
}

// ── Auth ──

#[derive(Deserialize, Debug)]
pub struct ChallengeResponse {
    pub nonce: Option<String>,
    pub salt: Option<String>,
    pub alg: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoginRequest {
    pub router_ip: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LoginResponse {
    pub ok: bool,
    pub message: String,
}

// ── Status ──

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct RouterStatus {
    pub wan: WanStatus,
    pub vpn: VpnStatus,
    pub wifi_ap: WifiApStatus,
    pub system: SystemStatus,
    pub traffic: TrafficStats,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TrafficStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_formatted: String,
    pub tx_formatted: String,
    pub total_formatted: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct WanStatus {
    pub connected: bool,
    #[serde(rename = "type")]
    pub wan_type: String,
    pub ip: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct VpnStatus {
    pub connected: bool,
    pub server_name: String,
    pub public_ip: String,
    pub country: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct WifiApStatus {
    pub ssid: String,
    pub clients: u32,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SystemStatus {
    pub uptime: u64,
    pub memory_used_pct: u32,
}

// ── WiFi ──

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: String,
    pub signal: i32,
    pub encryption: String,
    pub channel: u32,
}

#[derive(Deserialize, Debug)]
pub struct WifiConnectRequest {
    pub ssid: String,
    pub password: String,
    #[serde(default)]
    pub bssid: String,
}

// ── VPN ──

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VpnServer {
    pub id: String,
    pub name: String,
    pub city: String,
    pub ip: String,
    pub active: bool,
}

#[derive(Deserialize, Debug)]
pub struct VpnConnectRequest {
    pub server_id: String,
}

// ── NordVPN ──

#[derive(Deserialize, Debug)]
pub struct NordVpnTokenRequest {
    pub access_token: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NordVpnResult {
    pub servers_loaded: u32,
    pub servers: Vec<VpnServer>,
}

// ── Setup ──

#[derive(Deserialize, Debug)]
pub struct SetupWifiRequest {
    pub ssid: String,
    pub password: String,
    #[serde(default = "default_true")]
    pub band_24ghz: bool,
    #[serde(default = "default_true")]
    pub band_5ghz: bool,
}

#[derive(Deserialize, Debug)]
pub struct SetupSecurityRequest {
    #[serde(default = "default_true")]
    pub auto_connect: bool,
    #[serde(default = "default_true")]
    pub kill_switch: bool,
    #[serde(default = "default_true")]
    pub dns_protection: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Debug)]
pub struct SetupResult {
    pub step: String,
    pub success: bool,
    pub message: String,
}
