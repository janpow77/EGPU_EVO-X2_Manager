use crate::models::*;
use crate::router_api::RouterApi;
use reqwest::Client;
use serde_json::json;
use std::collections::HashSet;

pub struct NordVpnClient {
    client: Client,
}

struct WgServerConfig {
    name: String,
    city: String,
    server_ip: String,
    public_key: String,
}

impl NordVpnClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Fetch WireGuard credentials (private key) from NordVPN API.
    async fn get_private_key(&self, access_token: &str) -> Result<String, String> {
        let resp = self
            .client
            .get("https://api.nordvpn.com/v1/users/services/credentials")
            .basic_auth("token", Some(access_token))
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("NordVPN credentials request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!(
                "NordVPN API returned {}: Token invalid?",
                resp.status()
            ));
        }

        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse NordVPN response: {}", e))?;

        data["nordlynx_private_key"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or("No nordlynx_private_key in response".to_string())
    }

    /// Fetch recommended German WireGuard servers.
    async fn get_german_servers(&self) -> Result<Vec<WgServerConfig>, String> {
        let url = "https://api.nordvpn.com/v1/servers/recommendations?\
            filters[country_id]=81&\
            filters[servers_technologies][identifier]=wireguard_udp&\
            limit=10";

        let resp = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("NordVPN server list failed: {}", e))?;

        let servers: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse server list: {}", e))?;

        let mut result = Vec::new();
        let mut seen_cities = HashSet::new();

        for srv in &servers {
            // Extract city
            let city = srv
                .pointer("/locations/0/country/city/name")
                .or_else(|| srv.pointer("/locations/0/city/name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();

            if seen_cities.contains(&city) {
                continue;
            }

            // Find WireGuard public key
            let mut public_key = String::new();
            if let Some(techs) = srv["technologies"].as_array() {
                for tech in techs {
                    if tech["identifier"].as_str() == Some("wireguard_udp") {
                        if let Some(metadata) = tech["metadata"].as_array() {
                            for meta in metadata {
                                if meta["name"].as_str() == Some("public_key") {
                                    public_key =
                                        meta["value"].as_str().unwrap_or("").to_string();
                                }
                            }
                        }
                    }
                }
            }

            // Extract server IP
            let server_ip = srv["station"]
                .as_str()
                .map(|s| s.to_string())
                .or_else(|| {
                    srv["ips"]
                        .as_array()
                        .and_then(|ips| ips.first())
                        .and_then(|ip| ip["ip"]["ip"].as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();

            if public_key.is_empty() || server_ip.is_empty() {
                continue;
            }

            seen_cities.insert(city.clone());
            result.push(WgServerConfig {
                name: format!("DE-{}", city),
                city: city.clone(),
                server_ip,
                public_key,
            });

            if result.len() >= 4 {
                break;
            }
        }

        if result.is_empty() {
            return Err("No suitable German WireGuard servers found".to_string());
        }

        Ok(result)
    }

    /// Load NordVPN servers and deploy WireGuard configs to the router.
    pub async fn load_and_deploy(
        &self,
        access_token: &str,
        router: &RouterApi,
    ) -> Result<NordVpnResult, String> {
        // Step 1: Get private key
        let private_key = self.get_private_key(access_token).await?;

        // Step 2: Get German servers
        let servers = self.get_german_servers().await?;

        // Step 3: Deploy each server to router
        let mut deployed = Vec::new();

        for srv in &servers {
            let iface_name = format!(
                "wg_de_{}",
                srv.city.to_lowercase().replace(' ', "_").replace('-', "_")
            );

            // Write WireGuard config file
            let config = format!(
                "[Interface]\nPrivateKey = {}\nAddress = 10.5.0.2/16\n\n\
                 [Peer]\nPublicKey = {}\nAllowedIPs = 0.0.0.0/0\n\
                 Endpoint = {}:51820\nPersistentKeepalive = 25\n",
                private_key, srv.public_key, srv.server_ip
            );

            let conf_path = format!("/etc/wireguard/{}.conf", iface_name);
            router.file_write(&conf_path, &config).await?;

            // Configure UCI network interface
            router
                .ubus_call_uci_set_interface(
                    &iface_name,
                    &private_key,
                    &srv.public_key,
                    &srv.server_ip,
                )
                .await?;

            deployed.push(VpnServer {
                id: iface_name,
                name: srv.name.clone(),
                city: srv.city.clone(),
                ip: srv.server_ip.clone(),
                active: false,
            });
        }

        // Commit network config
        router.uci_commit("network").await?;

        // Configure firewall zone for VPN
        self.configure_firewall(router).await?;
        router.uci_commit("firewall").await?;

        Ok(NordVpnResult {
            servers_loaded: deployed.len() as u32,
            servers: deployed,
        })
    }

    async fn configure_firewall(&self, router: &RouterApi) -> Result<(), String> {
        // Add VPN zone to firewall - allow traffic from LAN through VPN
        // This is done via file.exec since UCI firewall config can be complex
        let commands = vec![
            "uci set firewall.vpn_zone=zone",
            "uci set firewall.vpn_zone.name=vpn",
            "uci set firewall.vpn_zone.input=REJECT",
            "uci set firewall.vpn_zone.output=ACCEPT",
            "uci set firewall.vpn_zone.forward=REJECT",
            "uci set firewall.vpn_zone.masq=1",
            "uci set firewall.vpn_zone.mtu_fix=1",
            "uci set firewall.vpn_fwd=forwarding",
            "uci set firewall.vpn_fwd.src=lan",
            "uci set firewall.vpn_fwd.dest=vpn",
        ];

        for cmd in commands {
            let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
            if parts.len() == 2 {
                let _ = router.file_exec(parts[0], &[parts[1]]).await;
            }
        }

        Ok(())
    }
}

// ubus_call_uci_set_interface is now in router_api.rs
