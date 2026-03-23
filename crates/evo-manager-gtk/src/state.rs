#![allow(dead_code)]

use serde::Deserialize;

/// Connection state to the EVO-X2.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connected,
    Connecting,
    Reconnecting(u32),
    Error(String),
}

/// Full widget state, updated by the polling loop.
#[derive(Debug, Clone)]
pub struct WidgetState {
    pub connection: ConnectionState,
    pub metrics: Option<EvoMetrics>,
}

impl Default for WidgetState {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Connecting,
            metrics: None,
        }
    }
}

impl WidgetState {
    /// Determine tray icon color based on service status.
    pub fn warning_color(&self) -> &'static str {
        match &self.metrics {
            Some(m) => {
                let active = m.services.values().filter(|s| s.as_str() == "active").count();
                let total = m.services.len();
                if total == 0 {
                    "gray"
                } else if active == total {
                    "green"
                } else if active > 0 {
                    "yellow"
                } else {
                    "red"
                }
            }
            None => "gray",
        }
    }

    pub fn active_count(&self) -> (usize, usize) {
        match &self.metrics {
            Some(m) => {
                let active = m.services.values().filter(|s| s.as_str() == "active").count();
                (active, m.services.len())
            }
            None => (0, 0),
        }
    }
}

// ─── API response types (from evo-metrics-server) ──────────────

#[derive(Debug, Clone, Deserialize)]
pub struct EvoMetrics {
    #[serde(default)]
    pub gtt: GttInfo,
    #[serde(default)]
    pub ram: MemInfo,
    #[serde(default)]
    pub cpu_load: CpuLoad,
    #[serde(default)]
    pub services: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GttInfo {
    #[serde(default)]
    pub used_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub used_gb: f64,
    #[serde(default)]
    pub total_gb: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemInfo {
    #[serde(default)]
    pub used_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub used_gb: f64,
    #[serde(default)]
    pub total_gb: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CpuLoad {
    #[serde(rename = "1min", default)]
    pub min1: f64,
    #[serde(rename = "5min", default)]
    pub min5: f64,
    #[serde(rename = "15min", default)]
    pub min15: f64,
}
