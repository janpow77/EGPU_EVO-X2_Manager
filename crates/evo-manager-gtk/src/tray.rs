use std::path::PathBuf;

use gtk::prelude::*;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use tracing::info;

const ICON_NAMES: &[(&str, &[u8])] = &[
    ("evo-green", include_bytes!("icons/green.svg")),
    ("evo-yellow", include_bytes!("icons/yellow.svg")),
    ("evo-red", include_bytes!("icons/red.svg")),
    ("evo-gray", include_bytes!("icons/gray.svg")),
];

fn ensure_icons() -> PathBuf {
    let icon_dir = dirs_icon_path();
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

fn dirs_icon_path() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local/share")
        });
    base.join("icons").join("evo-manager")
}

pub fn create_tray(
    on_status_click: impl Fn() + 'static,
    on_quit: impl Fn() + 'static,
) -> Option<AppIndicator> {
    let icon_dir = ensure_icons();
    let icon_dir_str = icon_dir.to_string_lossy().to_string();

    let mut indicator = AppIndicator::new("EVO-X2 Manager", "evo-gray");
    indicator.set_status(AppIndicatorStatus::Active);
    indicator.set_icon_theme_path(&icon_dir_str);
    indicator.set_icon_full("evo-gray", "EVO-X2 - Verbinde...");
    indicator.set_title("EVO-X2 Manager");

    let mut menu = gtk::Menu::new();

    let status_item = gtk::MenuItem::with_label("EVO-X2 Status anzeigen");
    status_item.connect_activate(move |_| on_status_click());
    menu.append(&status_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let quit_item = gtk::MenuItem::with_label("Beenden");
    quit_item.connect_activate(move |_| on_quit());
    menu.append(&quit_item);

    menu.show_all();
    indicator.set_menu(&mut menu);

    info!("Tray-Icon registriert");
    Some(indicator)
}

pub fn update_tray_icon(indicator: &mut AppIndicator, color: &str) {
    let icon_name = match color {
        "green" => "evo-green",
        "yellow" => "evo-yellow",
        "red" => "evo-red",
        _ => "evo-gray",
    };
    let description = match color {
        "green" => "EVO-X2 - Alle Services aktiv",
        "yellow" => "EVO-X2 - Teilweise aktiv",
        "red" => "EVO-X2 - Services offline",
        _ => "EVO-X2 - Nicht verbunden",
    };
    indicator.set_icon_full(icon_name, description);
}
