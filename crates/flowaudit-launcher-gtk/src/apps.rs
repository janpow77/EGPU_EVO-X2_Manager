use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct App {
    pub name: String,
    pub url: String,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

fn default_mode() -> String {
    "tab".into()
}

#[derive(Debug, Deserialize)]
struct Config {
    apps: Vec<App>,
}

pub fn icon_for(app: &App) -> String {
    app.icon
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| emoji_for(&app.name).to_string())
}

pub fn write(path: &Path, apps: &[App]) -> Result<()> {
    let mut out = String::new();
    out.push_str("# Desktop-App-Liste fuer den Generator regen.py\n");
    out.push_str("#\n");
    out.push_str("# mode:\n");
    out.push_str("#   app  -> Chrome --app=URL (eigenes Fenster, eigenes Profil)\n");
    out.push_str("#   tab  -> xdg-open URL (Standard-Browser-Tab)\n");
    out.push_str("#\n");
    out.push_str("# icon: optional, Emoji-String (Fallback: hardcoded Mapping)\n");
    out.push_str("# Diese Datei wird vom FlowAudit-Tray-Editor geschrieben.\n");
    out.push_str("\n");
    out.push_str("apps:\n");
    for app in apps {
        let mut parts = vec![
            format!("name: {}", yaml_value(&app.name)),
            format!("url: {}", yaml_value(&app.url)),
            format!("mode: {}", yaml_value(&app.mode)),
        ];
        if let Some(c) = app.comment.as_ref().filter(|s| !s.is_empty()) {
            parts.push(format!("comment: {}", yaml_value(c)));
        }
        if let Some(i) = app.icon.as_ref().filter(|s| !s.trim().is_empty()) {
            parts.push(format!("icon: {}", yaml_value(i)));
        }
        out.push_str(&format!("  - {{ {} }}\n", parts.join(", ")));
    }
    std::fs::write(path, out)
        .with_context(|| format!("apps.yml schreiben: {}", path.display()))?;
    Ok(())
}

fn yaml_value(s: &str) -> String {
    // Quote wenn nötig (Sonderzeichen, fängt mit # an, leerer String, Whitespace, Komma, etc.)
    let needs_quote = s.is_empty()
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.chars()
            .any(|c| matches!(c, '#' | ',' | ':' | '"' | '\'' | '{' | '}' | '[' | ']' | '\\' | '\n'));
    if needs_quote {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

pub fn default_apps_yml() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Projekte/flowlib/scripts/desktop-apps/apps.yml")
}

pub fn load(path: &Path) -> Result<Vec<App>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("apps.yml lesen: {}", path.display()))?;
    let cfg: Config = serde_yaml::from_str(&raw).context("apps.yml parsen")?;
    Ok(cfg.apps)
}

fn emoji_for(name: &str) -> &'static str {
    match name {
        "audit_designer" => "📊",
        "flowinvoice" => "💰",
        "hpp" => "📈",
        "qaaudit" => "✅",
        "love-ai" => "💕",
        "krypto" => "🔐",
        "flowsearch" => "🔍",
        "auditworkshop" => "🎓",
        "portainer" => "🐳",
        "jupyter" => "📓",
        "flower" => "🌺",
        "flowinvoice-stats" => "📉",
        "audit-designer-dev" => "⚙️",
        "riskanalysis" => "⚠️",
        _ => "🚀",
    }
}
