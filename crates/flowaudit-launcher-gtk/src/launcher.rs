use std::path::PathBuf;
use std::process::Command;

use crate::apps::App;

const LAUNCHER_URL: &str = "http://localhost:8888";

fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

fn chrome_bin() -> Option<PathBuf> {
    which::which("google-chrome")
        .or_else(|_| which::which("google-chrome-stable"))
        .ok()
}

fn chrome_profile_dir(name: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let dir = home.join(".config").join("chrome-apps").join(safe_name(name));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn launch(app: &App) {
    if app.mode == "app" {
        if let Some(chrome) = chrome_bin() {
            let profile = chrome_profile_dir(&app.name);
            let class = safe_name(&app.name);
            let result = Command::new(&chrome)
                .arg(format!("--app={}", app.url))
                .arg(format!("--user-data-dir={}", profile.display()))
                .arg(format!("--class={class}"))
                .spawn();
            if let Err(err) = result {
                tracing::warn!("Chrome-App {} konnte nicht starten: {err}", app.name);
            }
            return;
        }
        tracing::warn!("google-chrome nicht gefunden, falle auf xdg-open zurueck");
    }

    if let Err(err) = open::that(&app.url) {
        tracing::warn!("xdg-open {} fehlgeschlagen: {err}", app.url);
    }
}

pub fn open_launcher_web() {
    if let Err(err) = open::that(LAUNCHER_URL) {
        tracing::warn!("Launcher-Web {} oeffnen fehlgeschlagen: {err}", LAUNCHER_URL);
    }
}
