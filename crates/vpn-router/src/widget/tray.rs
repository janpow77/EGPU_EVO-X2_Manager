use std::path::PathBuf;

use gtk::prelude::*;
use libappindicator::{AppIndicator, AppIndicatorStatus};

const ICON_NAMES: &[(&str, &[u8])] = &[
    ("vpn-green", include_bytes!("icons/vpn-green.svg")),
    ("vpn-red", include_bytes!("icons/vpn-red.svg")),
    ("vpn-yellow", include_bytes!("icons/vpn-yellow.svg")),
    ("vpn-gray", include_bytes!("icons/vpn-gray.svg")),
];

#[derive(Clone, Debug)]
pub struct VpnState {
    pub vpn_connected: bool,
    pub wan_connected: bool,
    pub server_name: String,
    pub public_ip: String,
    pub country: String,
    pub traffic_rx: String,
    pub traffic_tx: String,
}

fn ensure_icons() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local/share")
        });
    let icon_dir = base.join("icons").join("vpn-router");
    if !icon_dir.exists() {
        std::fs::create_dir_all(&icon_dir).ok();
    }
    for (name, data) in ICON_NAMES {
        let path = icon_dir.join(format!("{name}.svg"));
        if !path.exists() {
            std::fs::write(&path, data).ok();
        }
    }
    icon_dir
}

pub fn create_tray(
    on_open_web: impl Fn() + 'static,
    on_quit: impl Fn() + 'static,
) -> Option<AppIndicator> {
    let icon_dir = ensure_icons();
    let icon_dir_str = icon_dir.to_string_lossy().to_string();

    let mut indicator = AppIndicator::new("VPN Router", "vpn-gray");
    indicator.set_status(AppIndicatorStatus::Active);
    indicator.set_icon_theme_path(&icon_dir_str);
    indicator.set_icon_full("vpn-gray", "VPN Router - Starte...");
    indicator.set_title("VPN Router");

    let mut menu = gtk::Menu::new();

    // Status items (will be updated)
    let vpn_item = gtk::MenuItem::with_label("VPN: --");
    vpn_item.set_sensitive(false);
    menu.append(&vpn_item);

    let ip_item = gtk::MenuItem::with_label("IP: --");
    ip_item.set_sensitive(false);
    menu.append(&ip_item);

    let traffic_item = gtk::MenuItem::with_label("Traffic: --");
    traffic_item.set_sensitive(false);
    menu.append(&traffic_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let web_item = gtk::MenuItem::with_label("Dashboard oeffnen");
    web_item.connect_activate(move |_| on_open_web());
    menu.append(&web_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let quit_item = gtk::MenuItem::with_label("Beenden");
    quit_item.connect_activate(move |_| on_quit());
    menu.append(&quit_item);

    menu.show_all();
    indicator.set_menu(&mut menu);

    Some(indicator)
}

pub fn update_tray(indicator: &mut AppIndicator, state: &VpnState) {
    // Update icon
    let (icon, desc) = if state.vpn_connected {
        ("vpn-green", format!("VPN: {} ({})", state.server_name, state.country))
    } else if state.wan_connected {
        ("vpn-yellow", "VPN getrennt - WAN verbunden".to_string())
    } else {
        ("vpn-red", "Offline".to_string())
    };
    indicator.set_icon_full(icon, &desc);

    // Update menu items via rebuilding (AppIndicator limitation)
    let mut menu = gtk::Menu::new();

    let vpn_label = if state.vpn_connected {
        format!("VPN: {} ({})", state.server_name, state.country)
    } else {
        "VPN: Getrennt".to_string()
    };
    let vpn_item = gtk::MenuItem::with_label(&vpn_label);
    vpn_item.set_sensitive(false);
    menu.append(&vpn_item);

    if !state.public_ip.is_empty() {
        let ip_item = gtk::MenuItem::with_label(&format!("IP: {}", state.public_ip));
        ip_item.set_sensitive(false);
        menu.append(&ip_item);
    }

    let traffic_item = gtk::MenuItem::with_label(&format!(
        "Traffic: {} / {}",
        state.traffic_rx, state.traffic_tx
    ));
    traffic_item.set_sensitive(false);
    menu.append(&traffic_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let web_item = gtk::MenuItem::with_label("Dashboard oeffnen");
    web_item.connect_activate(|_| {
        let _ = open::that("http://127.0.0.1:3000");
    });
    menu.append(&web_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let quit_item = gtk::MenuItem::with_label("Beenden");
    quit_item.connect_activate(|_| {
        gtk::main_quit();
    });
    menu.append(&quit_item);

    menu.show_all();
    indicator.set_menu(&mut menu);
}
