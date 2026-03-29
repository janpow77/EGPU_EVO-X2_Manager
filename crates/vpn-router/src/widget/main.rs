#[cfg(feature = "widget")]
mod tray;

#[cfg(feature = "widget")]
fn main() {
    use std::cell::RefCell;
    use std::rc::Rc;

    gtk::init().expect("GTK init failed");

    println!("VPN Router Widget startet");

    let (tx, rx) = async_channel::unbounded::<tray::VpnState>();

    // Tray icon
    let indicator = tray::create_tray(
        || {
            let _ = open::that("http://127.0.0.1:3080");
        },
        || {
            gtk::main_quit();
        },
    );

    let tray_indicator: Rc<RefCell<Option<libappindicator::AppIndicator>>> =
        Rc::new(RefCell::new(indicator));

    // Background polling thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            let client = reqwest::Client::new();
            loop {
                match client
                    .get("http://127.0.0.1:3080/api/status")
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if let Ok(data) = resp.json::<serde_json::Value>().await {
                            let vpn_on = data["vpn"]["connected"].as_bool().unwrap_or(false);
                            let wan_on = data["wan"]["connected"].as_bool().unwrap_or(false);
                            let server = data["vpn"]["server_name"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            let ip = data["vpn"]["public_ip"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            let country = data["vpn"]["country"]
                                .as_str()
                                .unwrap_or("")
                                .to_string();
                            let traffic_rx = data["traffic"]["rx_formatted"]
                                .as_str()
                                .unwrap_or("--")
                                .to_string();
                            let traffic_tx = data["traffic"]["tx_formatted"]
                                .as_str()
                                .unwrap_or("--")
                                .to_string();

                            let state = tray::VpnState {
                                vpn_connected: vpn_on,
                                wan_connected: wan_on,
                                server_name: server,
                                public_ip: ip,
                                country,
                                traffic_rx,
                                traffic_tx,
                            };
                            let _ = tx.send(state).await;
                        }
                    }
                    Err(_) => {
                        let _ = tx
                            .send(tray::VpnState {
                                vpn_connected: false,
                                wan_connected: false,
                                server_name: String::new(),
                                public_ip: String::new(),
                                country: String::new(),
                                traffic_rx: "--".into(),
                                traffic_tx: "--".into(),
                            })
                            .await;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    });

    // Receive state updates on GTK main loop
    let tray_update = Rc::clone(&tray_indicator);

    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        while let Ok(state) = rx.try_recv() {
            if let Some(ref mut ind) = *tray_update.borrow_mut() {
                tray::update_tray(ind, &state);
            }
        }
        glib::ControlFlow::Continue
    });

    println!("Widget gestartet - Tray-Icon aktiv");
    gtk::main();
}

#[cfg(not(feature = "widget"))]
fn main() {
    eprintln!("Widget feature not enabled. Build with: cargo build --features widget --bin vpn-router-widget");
}
