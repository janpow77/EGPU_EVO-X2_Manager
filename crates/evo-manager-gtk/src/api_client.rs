use std::time::Duration;

use anyhow::Result;
use tracing::debug;

use crate::config::EvoConfig;
use crate::state::{ConnectionState, EvoMetrics, WidgetState};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// Runs the polling loop, sending state updates through the channel.
pub async fn poll_loop(tx: async_channel::Sender<WidgetState>) {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("HTTP client");

    let mut consecutive_failures: u32 = 0;

    loop {
        let config = EvoConfig::load();
        let poll_interval = Duration::from_secs(config.poll_interval_secs);

        if config.evo_ip.is_empty() {
            let ws = WidgetState {
                connection: ConnectionState::Error("Keine IP konfiguriert".into()),
                ..Default::default()
            };
            if tx.send(ws).await.is_err() {
                break;
            }
            tokio::time::sleep(poll_interval).await;
            continue;
        }

        let state = fetch_metrics(&client, &config).await;

        match state {
            Ok(metrics) => {
                consecutive_failures = 0;
                let ws = WidgetState {
                    connection: ConnectionState::Connected,
                    metrics: Some(metrics),
                };
                if tx.send(ws).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                debug!("EVO-X2 Abfrage fehlgeschlagen ({consecutive_failures}): {e}");

                let conn = if consecutive_failures >= 5 {
                    ConnectionState::Error("EVO-X2 nicht erreichbar".into())
                } else {
                    ConnectionState::Reconnecting(consecutive_failures)
                };

                let ws = WidgetState {
                    connection: conn,
                    ..Default::default()
                };
                if tx.send(ws).await.is_err() {
                    break;
                }
            }
        }

        tokio::time::sleep(poll_interval).await;
    }
}

async fn fetch_metrics(client: &reqwest::Client, config: &EvoConfig) -> Result<EvoMetrics> {
    let url = config.metrics_url();
    let resp = client.get(&url).send().await?;
    let metrics: EvoMetrics = resp.json().await?;
    Ok(metrics)
}
