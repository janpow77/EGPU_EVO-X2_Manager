mod apps;
mod editor;
mod launcher;
mod tray;

use std::path::PathBuf;

use tracing::info;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    info!("FlowAudit Launcher Widget startet");

    let apps_yml: PathBuf = std::env::var("FLOWAUDIT_APPS_YML")
        .map(PathBuf::from)
        .unwrap_or_else(|_| apps::default_apps_yml());

    if !apps_yml.exists() {
        eprintln!(
            "apps.yml nicht gefunden: {}\nSetze FLOWAUDIT_APPS_YML oder lege die Datei am Default-Pfad ab.",
            apps_yml.display()
        );
        std::process::exit(1);
    }

    gtk::init().expect("GTK init");

    let _tray = tray::create(apps_yml).expect("Tray erstellen");

    gtk::main();
}
