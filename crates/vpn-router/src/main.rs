mod models;
mod nordvpn;
mod router_api;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use models::*;
use nordvpn::NordVpnClient;
use router_api::RouterApi;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

struct AppState {
    router_api: RouterApi,
    nordvpn: NordVpnClient,
}

#[tokio::main]
async fn main() {
    let router_ip =
        std::env::var("ROUTER_IP").unwrap_or_else(|_| "192.168.178.62".to_string());

    println!("VPN Travel Router Manager");
    println!("Router: {}", router_ip);
    println!("Starting on http://0.0.0.0:3080");

    let state = Arc::new(AppState {
        router_api: RouterApi::new(&router_ip),
        nordvpn: NordVpnClient::new(),
    });

    let app = Router::new()
        // Frontend
        .route("/", get(serve_frontend))
        // Auth
        .route("/api/auth/login", post(api_login))
        // Status
        .route("/api/status", get(api_status))
        .route("/api/board", get(api_board))
        // WiFi
        .route("/api/wifi/scan", post(api_wifi_scan))
        .route("/api/wifi/connect", post(api_wifi_connect))
        .route("/api/wifi/disconnect", post(api_wifi_disconnect))
        .route("/api/wifi/status", get(api_wifi_status))
        // VPN
        .route("/api/vpn/servers", get(api_vpn_servers))
        .route("/api/vpn/connect", post(api_vpn_connect))
        .route("/api/vpn/disconnect", post(api_vpn_disconnect))
        // NordVPN
        .route("/api/nordvpn/load-servers", post(api_nordvpn_load))
        // Setup
        .route("/api/setup/wifi-ap", post(api_setup_wifi))
        .route("/api/setup/security", post(api_setup_security))
        .route("/api/setup/test", post(api_setup_test))
        .route("/api/setup/emergency-restore", post(api_emergency_restore))
        .route("/api/setup/init-password", post(api_init_password))
        .layer(CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any))
        .with_state(state);

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3080".to_string());
    println!("Listening on http://{}", bind_addr);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .expect("Failed to bind");
    axum::serve(listener, app).await.expect("Server failed");
}

// ── Frontend ──

async fn serve_frontend() -> Html<&'static str> {
    Html(include_str!("../frontend/index.html"))
}

// ── Auth ──

async fn api_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    if !req.router_ip.is_empty() {
        state.router_api.set_router_ip(&req.router_ip).await;
    }

    match state.router_api.login(&req.password).await {
        Ok(()) => Json(LoginResponse {
            ok: true,
            message: "Login successful".to_string(),
        })
        .into_response(),
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                ok: false,
                message: e,
            }),
        )
            .into_response(),
    }
}

// ── Status ──

async fn api_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.router_api.get_status().await {
        Ok(status) => Json(status).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

async fn api_board(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.router_api.system_board().await {
        Ok(board) => Json(board).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ── WiFi ──

async fn api_wifi_scan(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.router_api.wifi_scan().await {
        Ok(networks) => Json(networks).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn api_wifi_connect(
    State(state): State<Arc<AppState>>,
    Json(req): Json<WifiConnectRequest>,
) -> impl IntoResponse {
    match state
        .router_api
        .wifi_connect(&req.ssid, &req.password, &req.bssid)
        .await
    {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn api_wifi_disconnect(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.router_api.wifi_disconnect().await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn api_wifi_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.router_api.wifi_status().await {
        Ok(status) => Json(status).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

// ── VPN ──

async fn api_vpn_servers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.router_api.vpn_list_servers().await {
        Ok(servers) => Json(servers).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn api_vpn_connect(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VpnConnectRequest>,
) -> impl IntoResponse {
    match state.router_api.vpn_connect(&req.server_id).await {
        Ok(()) => {
            Json(serde_json::json!({"ok": true, "message": "VPN connecting..."})).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn api_vpn_disconnect(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let servers = state
        .router_api
        .vpn_list_servers()
        .await
        .unwrap_or_default();
    for s in &servers {
        if s.active {
            let _ = state.router_api.vpn_disconnect(&s.id).await;
        }
    }
    Json(serde_json::json!({"ok": true}))
}

// ── NordVPN ──

async fn api_nordvpn_load(
    State(state): State<Arc<AppState>>,
    Json(req): Json<NordVpnTokenRequest>,
) -> impl IntoResponse {
    if !state.router_api.is_authenticated().await {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "Not logged in to router"})),
        )
            .into_response();
    }

    match state
        .nordvpn
        .load_and_deploy(&req.access_token, &state.router_api)
        .await
    {
        Ok(result) => Json(result).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

// ── Setup ──

async fn api_setup_wifi(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupWifiRequest>,
) -> impl IntoResponse {
    if req.password.len() < 8 {
        return Json(SetupResult {
            step: "wifi-ap".into(),
            success: false,
            message: "Password must be at least 8 characters".into(),
        });
    }

    let api = &state.router_api;

    let result = async {
        let resp = api
            .ubus_call_uci_get_wifi_ifaces()
            .await?;

        if let Some(values) = resp.as_object() {
            for (section, iface) in values {
                if iface["mode"].as_str() == Some("ap") {
                    api.uci_set("wireless", section, "ssid", serde_json::json!(req.ssid))
                        .await?;
                    api.uci_set("wireless", section, "key", serde_json::json!(req.password))
                        .await?;
                    api.uci_set(
                        "wireless",
                        section,
                        "encryption",
                        serde_json::json!("psk2+ccmp"),
                    )
                    .await?;
                }
            }
        }

        api.uci_commit("wireless").await?;
        api.file_exec("/sbin/wifi", &["reload"]).await?;
        Ok::<(), String>(())
    }
    .await;

    match result {
        Ok(()) => Json(SetupResult {
            step: "wifi-ap".into(),
            success: true,
            message: format!("WiFi AP '{}' configured", req.ssid),
        }),
        Err(e) => Json(SetupResult {
            step: "wifi-ap".into(),
            success: false,
            message: e,
        }),
    }
}

async fn api_setup_security(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupSecurityRequest>,
) -> impl IntoResponse {
    let api = &state.router_api;
    let mut messages = Vec::new();

    if req.kill_switch {
        let _ = api
            .file_exec(
                "/bin/sh",
                &[
                    "-c",
                    "uci set firewall.vpn_kill=rule; \
                     uci set firewall.vpn_kill.name='Kill Switch'; \
                     uci set firewall.vpn_kill.src='lan'; \
                     uci set firewall.vpn_kill.dest='wan'; \
                     uci set firewall.vpn_kill.proto='all'; \
                     uci set firewall.vpn_kill.target='REJECT'; \
                     uci commit firewall; \
                     /etc/init.d/firewall reload",
                ],
            )
            .await;
        messages.push("Kill Switch: ON");
    }

    if req.dns_protection {
        let _ = api
            .file_exec(
                "/bin/sh",
                &[
                    "-c",
                    "uci set dhcp.@dnsmasq[0].noresolv='1'; \
                     uci delete dhcp.@dnsmasq[0].server 2>/dev/null; \
                     uci add_list dhcp.@dnsmasq[0].server='103.86.96.100'; \
                     uci add_list dhcp.@dnsmasq[0].server='103.86.99.100'; \
                     uci commit dhcp; \
                     /etc/init.d/dnsmasq restart",
                ],
            )
            .await;
        messages.push("DNS Protection: ON");
    }

    Json(SetupResult {
        step: "security".into(),
        success: true,
        message: messages.join(", "),
    })
}

async fn api_setup_test(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut results = Vec::new();

    // Test 1: Router reachable
    match state.router_api.system_board().await {
        Ok(_) => results.push(serde_json::json!({"test": "Router", "pass": true})),
        Err(e) => {
            results.push(serde_json::json!({"test": "Router", "pass": false, "error": e}))
        }
    }

    // Test 2-4: Full status
    let status = state.router_api.get_status().await.unwrap_or_default();

    results.push(serde_json::json!({
        "test": "WAN",
        "pass": status.wan.connected,
        "detail": format!("{} ({})", status.wan.wan_type, status.wan.ip)
    }));

    results.push(serde_json::json!({
        "test": "VPN",
        "pass": status.vpn.connected,
        "detail": status.vpn.server_name
    }));

    results.push(serde_json::json!({
        "test": "IP",
        "pass": status.vpn.country == "Germany",
        "detail": format!("{} ({})", status.vpn.public_ip, status.vpn.country)
    }));

    Json(results)
}

// ── Emergency Restore (after factory reset) ──

async fn api_init_password(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let ip = if req.router_ip.is_empty() { "192.168.8.1" } else { &req.router_ip };
    state.router_api.set_router_ip(ip).await;

    // After factory reset, SSH has no password. Set the new one.
    let set_pw_cmd = format!(
        "echo -e '{pw}\\n{pw}' | passwd root",
        pw = req.password.replace('\'', "'\\''")
    );

    // Try with empty password first (fresh reset)
    let output = tokio::process::Command::new("sshpass")
        .args(["-p", "", "ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=5",
               &format!("root@{}", ip), &set_pw_cmd])
        .output().await;

    let success = output.map(|o| o.status.success()).unwrap_or(false);

    if success {
        // Install SSH key
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let key_path = format!("{}/.ssh/glinet_key.pub", home);
        if let Ok(pubkey) = tokio::fs::read_to_string(&key_path).await {
            let install_cmd = format!(
                "mkdir -p /etc/dropbear; echo '{}' >> /etc/dropbear/authorized_keys; chmod 600 /etc/dropbear/authorized_keys",
                pubkey.trim()
            );
            let _ = tokio::process::Command::new("sshpass")
                .args(["-p", &req.password, "ssh", "-o", "StrictHostKeyChecking=no",
                       &format!("root@{}", ip), &install_cmd])
                .output().await;
        }

        Json(serde_json::json!({"ok": true, "message": "Password set, SSH key installed"}))
    } else {
        // Maybe password was already set - try login with provided password
        match state.router_api.login(&req.password).await {
            Ok(()) => Json(serde_json::json!({"ok": true, "message": "Already configured, logged in"})),
            Err(e) => Json(serde_json::json!({"ok": false, "message": e})),
        }
    }
}

async fn api_emergency_restore(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let api = &state.router_api;
    let mut steps = Vec::new();

    // Step 1: Disable captive portal
    match api.file_exec("/bin/sh", &["-c",
        "uci set glconfig.general.init_status='1' 2>/dev/null; \
         uci set glconfig.general.init_pwd='1' 2>/dev/null; \
         uci commit glconfig 2>/dev/null; \
         for name in capture22 capture443 captureport; do \
           section=$(uci show firewall 2>/dev/null | grep \"name='$name'\" | cut -d. -f2 | cut -d= -f1); \
           [ -n \"$section\" ] && uci delete firewall.$section; \
         done; \
         uci commit firewall; \
         echo OK"
    ]).await {
        Ok(o) => steps.push(serde_json::json!({"step": "Captive Portal", "ok": o.contains("OK")})),
        Err(e) => steps.push(serde_json::json!({"step": "Captive Portal", "ok": false, "error": e})),
    }

    // Step 2: Restore WireGuard config from saved UCI
    let configs_dir = std::env::current_dir()
        .map(|p| p.join("configs/wireguard-uci.txt"))
        .unwrap_or_default();

    if let Ok(uci_config) = tokio::fs::read_to_string(&configs_dir).await {
        let mut ok = true;
        for line in uci_config.lines() {
            if line.trim().is_empty() { continue; }
            let cmd = format!("uci set {}", line.trim());
            if api.file_exec("/bin/sh", &["-c", &cmd]).await.is_err() {
                ok = false;
            }
        }
        let _ = api.file_exec("/bin/sh", &["-c", "uci commit network"]).await;
        steps.push(serde_json::json!({"step": "WireGuard VPN", "ok": ok}));
    } else {
        steps.push(serde_json::json!({"step": "WireGuard VPN", "ok": false, "error": "No backup found"}));
    }

    // Step 3: DNS
    match api.file_exec("/bin/sh", &["-c",
        "uci set dhcp.@dnsmasq[0].noresolv='1'; \
         uci delete dhcp.@dnsmasq[0].server 2>/dev/null; \
         uci add_list dhcp.@dnsmasq[0].server='103.86.96.100'; \
         uci add_list dhcp.@dnsmasq[0].server='103.86.99.100'; \
         uci set dhcp.@dnsmasq[0].rebind_protection='0'; \
         uci commit dhcp; /etc/init.d/dnsmasq restart; echo OK"
    ]).await {
        Ok(o) => steps.push(serde_json::json!({"step": "DNS", "ok": o.contains("OK")})),
        Err(e) => steps.push(serde_json::json!({"step": "DNS", "ok": false, "error": e})),
    }

    // Step 4: Firewall VPN zone
    match api.file_exec("/bin/sh", &["-c",
        "uci set firewall.vpn_zone=zone; \
         uci set firewall.vpn_zone.name='vpn'; \
         uci set firewall.vpn_zone.input='REJECT'; \
         uci set firewall.vpn_zone.output='ACCEPT'; \
         uci set firewall.vpn_zone.forward='REJECT'; \
         uci set firewall.vpn_zone.masq='1'; \
         uci set firewall.vpn_zone.mtu_fix='1'; \
         uci set firewall.vpn_zone.network='wg_de_frankfurt'; \
         uci set firewall.vpn_fwd=forwarding; \
         uci set firewall.vpn_fwd.src='lan'; \
         uci set firewall.vpn_fwd.dest='vpn'; \
         uci commit firewall; \
         /etc/init.d/firewall reload 2>/dev/null; echo OK"
    ]).await {
        Ok(o) => steps.push(serde_json::json!({"step": "Firewall", "ok": o.contains("OK")})),
        Err(e) => steps.push(serde_json::json!({"step": "Firewall", "ok": false, "error": e})),
    }

    // Step 5: Reload network and start VPN
    let _ = api.file_exec("/bin/sh", &["-c", "/etc/init.d/network reload; sleep 2; ifup wg_de_frankfurt"]).await;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let wg_ok = api.file_exec("wg", &["show"]).await
        .map(|o| o.contains("latest handshake"))
        .unwrap_or(false);
    steps.push(serde_json::json!({"step": "VPN Start", "ok": wg_ok}));

    Json(steps)
}
