mod metrics;
mod ocr;
mod webhook;

use clap::{Parser, Subcommand};
use tracing::info;

#[derive(Parser)]
#[command(name = "evo-x2-services", about = "EVO-X2 Server-Dienste")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// System-Metriken Server (GTT, RAM, CPU, Service-Status) auf Port 8084
    Metrics {
        #[arg(short, long, default_value = "0.0.0.0")]
        host: String,
        #[arg(short, long, default_value_t = 8084)]
        port: u16,
    },
    /// GitHub Webhook Receiver auf Port 9000
    Webhook {
        #[arg(short, long, default_value = "0.0.0.0")]
        host: String,
        #[arg(short, long, default_value_t = 9000)]
        port: u16,
    },
    /// OCR Server (Docling-Wrapper) auf Port 8083
    Ocr {
        #[arg(short, long, default_value = "0.0.0.0")]
        host: String,
        #[arg(short, long, default_value_t = 8083)]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Metrics { host, port } => {
            info!("Starte Metrics-Server auf {host}:{port}");
            metrics::serve(&host, port).await
        }
        Commands::Webhook { host, port } => {
            info!("Starte Webhook-Receiver auf {host}:{port}");
            webhook::serve(&host, port).await
        }
        Commands::Ocr { host, port } => {
            info!("Starte OCR-Server auf {host}:{port}");
            ocr::serve(&host, port).await
        }
    }
}
