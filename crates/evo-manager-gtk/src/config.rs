use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvoConfig {
    #[serde(default = "default_ip")]
    pub evo_ip: String,
    #[serde(default = "default_port")]
    pub metrics_port: u16,
    #[serde(default = "default_user")]
    pub ssh_user: String,
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub github_url: String,
    #[serde(default)]
    pub setup_dir: String,
}

fn default_ip() -> String { String::new() }
fn default_port() -> u16 { 8084 }
fn default_user() -> String { "jan".into() }
fn default_poll() -> u64 { 5 }

impl Default for EvoConfig {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        Self {
            evo_ip: default_ip(),
            metrics_port: default_port(),
            ssh_user: default_user(),
            poll_interval_secs: default_poll(),
            github_url: String::new(),
            setup_dir: format!("{home}/Projekte/evo/setup"),
        }
    }
}

impl EvoConfig {
    pub fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        base.join("evo-manager").join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn metrics_url(&self) -> String {
        format!("http://{}:{}/metrics", self.evo_ip, self.metrics_port)
    }
}
