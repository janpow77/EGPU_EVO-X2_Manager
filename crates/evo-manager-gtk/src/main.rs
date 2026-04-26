mod api_client;
mod config;
mod popup;
mod state;
mod tray;

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use tracing::info;

use crate::state::WidgetState;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    info!("EVO-X2 Manager Widget startet");

    if gtk::init().is_err() {
        eprintln!("FEHLER: GTK konnte nicht initialisiert werden.");
        eprintln!("Ist ein Display vorhanden? (DISPLAY/WAYLAND_DISPLAY)");
        std::process::exit(1);
    }

    // Create popup window (hidden)
    let popup_window = popup::build_popup();
    let popup_ref = Rc::new(RefCell::new(popup_window));

    // Channel: tokio -> GTK main loop
    let (tx, rx) = async_channel::unbounded::<WidgetState>();

    // Tray icon
    let popup_toggle = Rc::clone(&popup_ref);
    let tray_indicator: Rc<RefCell<Option<libappindicator::AppIndicator>>> =
        Rc::new(RefCell::new(None));

    let indicator = tray::create_tray(
        move || {
            let win = popup_toggle.borrow();
            if win.is_visible() {
                win.hide();
            } else {
                win.show_all();
                win.present();
            }
        },
        || {
            gtk::main_quit();
        },
    );

    *tray_indicator.borrow_mut() = indicator;

    // Background polling thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(api_client::poll_loop(tx));
    });

    // Receive state updates on GTK main loop
    let popup_update = Rc::clone(&popup_ref);
    let tray_update = Rc::clone(&tray_indicator);
    let last_color: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let last_state: Rc<RefCell<Option<WidgetState>>> = Rc::new(RefCell::new(None));

    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        // Channel komplett drainieren, nur den letzten State behalten
        let mut latest: Option<WidgetState> = None;
        while let Ok(state) = rx.try_recv() {
            latest = Some(state);
        }

        if let Some(state) = latest {
            // Nur updaten wenn sich der State tatsächlich geändert hat
            let changed = {
                let prev = last_state.borrow();
                prev.as_ref() != Some(&state)
            };

            if changed {
                // Tray-Icon Farbe
                let color = state.warning_color().to_string();
                {
                    let mut lc = last_color.borrow_mut();
                    if *lc != color {
                        if let Some(ref mut ind) = *tray_update.borrow_mut() {
                            tray::update_tray_icon(ind, &color);
                        }
                        *lc = color;
                    }
                }

                // Popup nur wenn sichtbar UND nicht aktiv (User interagiert nicht gerade
                // damit). Verhindert Tab-Springen und Scroll-Verlust waehrend Bedienung.
                // Sobald Fokus weg ist (Klick anderswo), kommen Updates wieder.
                let win = popup_update.borrow();
                if win.is_visible() && !win.is_active() {
                    popup::update_popup(&win, &state);
                }

                *last_state.borrow_mut() = Some(state);
            }
        }
        glib::ControlFlow::Continue
    });

    info!("Widget gestartet — Tray-Icon aktiv");

    gtk::main();
}
