use crate::models::*;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use tokio::process::Command;

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Sanitize a value for use inside single-quoted shell strings.
/// Removes single quotes and common shell metacharacters.
fn sanitize_shell(input: &str) -> String {
    input
        .replace('\'', "")
        .replace('\\', "")
        .replace('`', "")
        .replace('$', "")
        .replace('(', "")
        .replace(')', "")
        .replace(';', "")
        .replace('|', "")
        .replace('&', "")
        .replace('\n', "")
        .replace('\r', "")
}

/// Validate an identifier (config name, interface name, UCI section).
/// Only allows alphanumeric, underscore, hyphen.
fn sanitize_identifier(input: &str) -> Result<String, String> {
    let clean: String = input.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
        .collect();
    if clean.is_empty() || clean.len() > 64 {
        return Err(format!("Invalid identifier: '{}'", input));
    }
    Ok(clean)
}
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct RouterApi {
    client: Client,
    state: Arc<RwLock<RouterState>>,
}

struct RouterState {
    router_ip: String,
    ssh_key: String,
    authenticated: bool,
}

impl RouterApi {
    pub fn new(router_ip: &str) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        Self {
            client: Client::new(),
            state: Arc::new(RwLock::new(RouterState {
                router_ip: router_ip.to_string(),
                ssh_key: format!("{}/.ssh/glinet_key", home),
                authenticated: false,
            })),
        }
    }

    pub async fn set_router_ip(&self, ip: &str) {
        let mut state = self.state.write().await;
        state.router_ip = ip.to_string();
    }

    // ── SSH command execution ──

    async fn ssh_cmd(&self, cmd: &str) -> Result<String, String> {
        let state = self.state.read().await;
        let output = Command::new("ssh")
            .args([
                "-i", &state.ssh_key,
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                &format!("root@{}", state.router_ip),
                cmd,
            ])
            .output()
            .await
            .map_err(|e| format!("SSH exec error: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Try with password fallback via sshpass
            return self.ssh_cmd_with_password(cmd).await
                .map_err(|_| format!("SSH failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn ssh_cmd_with_password(&self, cmd: &str) -> Result<String, String> {
        let state = self.state.read().await;
        let password = std::env::var("ROUTER_PASSWORD").unwrap_or_default();
        if password.is_empty() {
            return Err("No ROUTER_PASSWORD set and SSH key failed".to_string());
        }
        let output = Command::new("sshpass")
            .args([
                "-p", &password,
                "ssh",
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                &format!("root@{}", state.router_ip),
                cmd,
            ])
            .output()
            .await
            .map_err(|e| format!("sshpass exec error: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("SSH auth failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Execute a ubus call via SSH and return parsed JSON.
    pub async fn ubus_call(
        &self,
        service: &str,
        method: &str,
        args: serde_json::Value,
    ) -> Result<UbusResponse, String> {
        let args_str = serde_json::to_string(&args).unwrap_or_else(|_| "{}".into());
        let cmd = format!("ubus call {} {} '{}'", service, method, args_str);
        let output = self.ssh_cmd(&cmd).await?;

        if output.trim().is_empty() {
            // ubus call succeeded but returned no data (e.g., interface up/down)
            return Ok(UbusResponse {
                jsonrpc: "2.0".into(),
                id: 1,
                result: Some(json!([0, {}])),
                error: None,
            });
        }

        let data: serde_json::Value = serde_json::from_str(output.trim())
            .map_err(|e| format!("JSON parse error: {} (output: {})", e, &output[..output.len().min(200)]))?;

        Ok(UbusResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(json!([0, data])),
            error: None,
        })
    }

    // ── Authentication ──

    pub async fn login(&self, password: &str) -> Result<(), String> {
        let state_r = self.state.read().await;
        let ip = state_r.router_ip.clone();
        drop(state_r);

        // Test SSH with password
        let output = Command::new("sshpass")
            .args([
                "-p", password,
                "ssh",
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                &format!("root@{}", ip),
                "echo OK",
            ])
            .output()
            .await
            .map_err(|e| format!("SSH error: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.contains("OK") {
            return Err("Login failed - wrong password".to_string());
        }

        // Copy SSH key for future passwordless access
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let key_path = format!("{}/.ssh/glinet_key.pub", home);
        if let Ok(pubkey) = tokio::fs::read_to_string(&key_path).await {
            let install_cmd = format!(
                "mkdir -p /etc/dropbear; echo '{}' >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys",
                pubkey.trim()
            );
            let _ = Command::new("sshpass")
                .args([
                    "-p", password,
                    "ssh", "-o", "StrictHostKeyChecking=no",
                    &format!("root@{}", ip),
                    &install_cmd,
                ])
                .output()
                .await;
        }

        let mut state = self.state.write().await;
        state.authenticated = true;
        Ok(())
    }

    pub async fn is_authenticated(&self) -> bool {
        // Try a quick SSH command to verify
        self.ssh_cmd("echo 1").await.is_ok()
    }

    // ── Status ──

    pub async fn get_status(&self) -> Result<RouterStatus, String> {
        let mut status = RouterStatus::default();

        // Network interfaces
        if let Ok(resp) = self.ubus_call("network.interface", "dump", json!({})).await {
            if let Some(data) = resp.data() {
                if let Some(interfaces) = data["interface"].as_array() {
                    for iface in interfaces {
                        let name = iface["interface"].as_str().unwrap_or("");
                        let up = iface["up"].as_bool().unwrap_or(false);

                        if name == "wan" && up {
                            status.wan.connected = true;
                            status.wan.wan_type = "ethernet".to_string();
                            if let Some(addrs) = iface["ipv4-address"].as_array() {
                                if let Some(addr) = addrs.first() {
                                    status.wan.ip = addr["address"].as_str().unwrap_or("").to_string();
                                }
                            }
                        }
                        if name == "wwan" && up {
                            status.wan.connected = true;
                            status.wan.wan_type = "repeater".to_string();
                        }
                        if name.starts_with("wg_") && up {
                            status.vpn.connected = true;
                            status.vpn.server_name = name.replace("wg_", "DE-");
                        }
                    }
                }
            }
        }

        // System info
        if let Ok(resp) = self.ubus_call("system", "info", json!({})).await {
            if let Some(data) = resp.data() {
                status.system.uptime = data["uptime"].as_u64().unwrap_or(0);
                if let Some(mem) = data["memory"].as_object() {
                    let total = mem.get("total").and_then(|v| v.as_u64()).unwrap_or(1);
                    let free = mem.get("free").and_then(|v| v.as_u64()).unwrap_or(0);
                    let buffers = mem.get("buffered").and_then(|v| v.as_u64()).unwrap_or(0);
                    let used = total.saturating_sub(free).saturating_sub(buffers);
                    status.system.memory_used_pct = ((used * 100) / total) as u32;
                }
            }
        }

        // WiFi clients
        if let Ok(resp) = self.ubus_call("hostapd.wlan0", "get_clients", json!({})).await {
            if let Some(data) = resp.data() {
                if let Some(clients) = data["clients"].as_object() {
                    status.wifi_ap.clients = clients.len() as u32;
                }
            }
        }

        // WiFi SSID
        if let Ok(output) = self.ssh_cmd("uci get wireless.default_radio0.ssid 2>/dev/null || uci get wireless.@wifi-iface[0].ssid 2>/dev/null").await {
            status.wifi_ap.ssid = output.trim().to_string();
        }

        // Traffic stats from WireGuard
        if let Ok(output) = self.ssh_cmd("wg show all transfer 2>/dev/null").await {
            for line in output.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 4 {
                    let rx: u64 = parts[2].trim().parse().unwrap_or(0);
                    let tx: u64 = parts[3].trim().parse().unwrap_or(0);
                    status.traffic.rx_bytes += rx;
                    status.traffic.tx_bytes += tx;
                }
            }
            status.traffic.rx_formatted = format_bytes(status.traffic.rx_bytes);
            status.traffic.tx_formatted = format_bytes(status.traffic.tx_bytes);
            status.traffic.total_formatted = format_bytes(status.traffic.rx_bytes + status.traffic.tx_bytes);
        }

        // Public IP
        if status.wan.connected || status.vpn.connected {
            if let Ok(resp) = self.client.get("https://api.ipify.org?format=json")
                .timeout(std::time::Duration::from_secs(5))
                .send().await
            {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    if let Some(ip) = data["ip"].as_str() {
                        status.vpn.public_ip = ip.to_string();
                    }
                }
            }
            if !status.vpn.public_ip.is_empty() {
                let geo_url = format!("https://ipapi.co/{}/json/", status.vpn.public_ip);
                if let Ok(resp) = self.client.get(&geo_url)
                    .timeout(std::time::Duration::from_secs(5))
                    .send().await
                {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        status.vpn.country = data["country_name"].as_str().unwrap_or("").to_string();
                    }
                }
            }
        }

        Ok(status)
    }

    // ── WiFi Repeater ──

    pub async fn wifi_scan(&self) -> Result<Vec<WifiNetwork>, String> {
        let resp = self.ubus_call("repeater", "scan", json!({})).await?;
        let data = resp.data().ok_or("No scan data")?;

        let mut networks = Vec::new();
        let list = data["list"].as_array()
            .or_else(|| data["results"].as_array())
            .or_else(|| data.as_array());

        if let Some(items) = list {
            for item in items {
                networks.push(WifiNetwork {
                    ssid: item["ssid"].as_str().unwrap_or("").to_string(),
                    bssid: item["bssid"].as_str().unwrap_or("").to_string(),
                    signal: item["signal"].as_i64().unwrap_or(0) as i32,
                    encryption: item["encryption"].as_str()
                        .or_else(|| item["security"].as_str())
                        .unwrap_or("unknown").to_string(),
                    channel: item["channel"].as_u64().unwrap_or(0) as u32,
                });
            }
        }

        networks.sort_by(|a, b| b.signal.cmp(&a.signal));
        Ok(networks)
    }

    pub async fn wifi_connect(&self, ssid: &str, password: &str, _bssid: &str) -> Result<(), String> {
        let safe_ssid = sanitize_shell(ssid);
        let safe_pw = sanitize_shell(password);
        let cmd = format!(
            "ubus call repeater connect '{{\"ssid\":\"{}\",\"key\":\"{}\"}}'",
            safe_ssid, safe_pw
        );
        self.ssh_cmd(&cmd).await?;
        Ok(())
    }

    pub async fn wifi_disconnect(&self) -> Result<(), String> {
        self.ssh_cmd("ubus call repeater disconnect '{}'").await?;
        Ok(())
    }

    pub async fn wifi_status(&self) -> Result<serde_json::Value, String> {
        let output = self.ssh_cmd("ubus call repeater status '{}'").await?;
        serde_json::from_str(output.trim())
            .map_err(|e| format!("Parse error: {}", e))
    }

    // ── VPN / WireGuard ──

    pub async fn vpn_list_servers(&self) -> Result<Vec<VpnServer>, String> {
        let output = self.ssh_cmd(
            "uci show network | grep \"proto='wireguard'\" | cut -d. -f2"
        ).await?;

        let mut servers = Vec::new();
        for line in output.lines() {
            let name = line.trim();
            if name.is_empty() { continue; }
            let is_up = self.ssh_cmd(&format!(
                "ubus call network.interface.{} status '{{}}' 2>/dev/null | jsonfilter -e '@.up'", name
            )).await.map(|o| o.trim() == "true").unwrap_or(false);

            servers.push(VpnServer {
                id: name.to_string(),
                name: name.replace("wg_", "DE-").replace('_', " "),
                city: name.replace("wg_de_", "").replace('_', " "),
                ip: String::new(),
                active: is_up,
            });
        }
        Ok(servers)
    }

    pub async fn vpn_connect(&self, server_id: &str) -> Result<(), String> {
        let safe_id = sanitize_identifier(server_id)?;
        let _ = self.ssh_cmd("for i in $(uci show network | grep \"proto='wireguard'\" | cut -d. -f2); do ifdown $i 2>/dev/null; done").await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        self.ssh_cmd(&format!("ifup {}", safe_id)).await?;
        Ok(())
    }

    pub async fn vpn_disconnect(&self, server_id: &str) -> Result<(), String> {
        let safe_id = sanitize_identifier(server_id)?;
        self.ssh_cmd(&format!("ifdown {}", safe_id)).await?;
        Ok(())
    }

    // ── UCI + File helpers ──

    pub async fn uci_set(&self, config: &str, section: &str, key: &str, value: serde_json::Value) -> Result<(), String> {
        let safe_config = sanitize_identifier(config)?;
        let safe_section = sanitize_identifier(section)?;
        let safe_key = sanitize_identifier(key)?;
        let val_str = match &value {
            serde_json::Value::String(s) => sanitize_shell(s),
            other => sanitize_shell(&other.to_string()),
        };
        self.ssh_cmd(&format!("uci set {}.{}.{}='{}'", safe_config, safe_section, safe_key, val_str)).await?;
        Ok(())
    }

    pub async fn uci_commit(&self, config: &str) -> Result<(), String> {
        self.ssh_cmd(&format!("uci commit {}", config)).await?;
        Ok(())
    }

    pub async fn file_write(&self, path: &str, data: &str) -> Result<(), String> {
        // Write via SSH heredoc
        let cmd = format!("cat > {} << 'WGEOF'\n{}\nWGEOF", path, data);
        self.ssh_cmd(&cmd).await?;
        Ok(())
    }

    pub async fn file_exec(&self, command: &str, params: &[&str]) -> Result<String, String> {
        let cmd = format!("{} {}", command, params.join(" "));
        self.ssh_cmd(&cmd).await
    }

    pub async fn system_board(&self) -> Result<serde_json::Value, String> {
        let output = self.ssh_cmd("ubus call system board").await?;
        serde_json::from_str(output.trim())
            .map_err(|e| format!("Parse error: {}", e))
    }

    pub async fn ubus_call_uci_get_wifi_ifaces(&self) -> Result<serde_json::Value, String> {
        let output = self.ssh_cmd("uci show wireless 2>/dev/null | grep wifi-iface").await?;
        // Parse UCI output into structured data
        let mut ifaces = serde_json::Map::new();
        for line in output.lines() {
            // Format: wireless.default_radio0=wifi-iface
            if let Some(section) = line.split('=').next() {
                let section_name = section.replace("wireless.", "");
                if !ifaces.contains_key(&section_name) {
                    // Get mode and ssid for this interface
                    let mode = self.ssh_cmd(&format!("uci get wireless.{}.mode 2>/dev/null", section_name)).await.unwrap_or_default();
                    let ssid = self.ssh_cmd(&format!("uci get wireless.{}.ssid 2>/dev/null", section_name)).await.unwrap_or_default();
                    ifaces.insert(section_name, json!({
                        "mode": mode.trim(),
                        "ssid": ssid.trim()
                    }));
                }
            }
        }
        Ok(serde_json::Value::Object(ifaces))
    }

    /// Helper to set up a WireGuard interface via UCI.
    pub async fn ubus_call_uci_set_interface(
        &self,
        iface_name: &str,
        private_key: &str,
        public_key: &str,
        endpoint_ip: &str,
    ) -> Result<(), String> {
        let commands = format!(
            "uci set network.{iface}=interface; \
             uci set network.{iface}.proto='wireguard'; \
             uci set network.{iface}.private_key='{privkey}'; \
             uci add_list network.{iface}.addresses='10.5.0.2/16'; \
             uci add_list network.{iface}.dns='103.86.96.100'; \
             uci add_list network.{iface}.dns='103.86.99.100'; \
             uci set network.{iface}_peer=wireguard_{iface}; \
             uci set network.{iface}_peer.public_key='{pubkey}'; \
             uci set network.{iface}_peer.endpoint_host='{endpoint}'; \
             uci set network.{iface}_peer.endpoint_port='51820'; \
             uci add_list network.{iface}_peer.allowed_ips='0.0.0.0/0'; \
             uci set network.{iface}_peer.persistent_keepalive='25'; \
             uci set network.{iface}_peer.route_allowed_ips='1'",
            iface = iface_name,
            privkey = private_key,
            pubkey = public_key,
            endpoint = endpoint_ip,
        );
        self.ssh_cmd(&commands).await?;
        Ok(())
    }
}
