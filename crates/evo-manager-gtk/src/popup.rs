use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{
    Align, Box as GtkBox, Button, CssProvider, Entry, Image, Label, LevelBar, Notebook,
    Orientation, Separator, StyleContext, Window, WindowType, ScrolledWindow,
};

use crate::config::EvoConfig;
use crate::state::{ConnectionState, WidgetState};

thread_local! {
    static ACTIVE_TAB: Rc<Cell<u32>> = Rc::new(Cell::new(0));
}

const CSS: &str = r#"
window {
    background-color: #1a1a18;
    color: #e8e7e0;
}
.popup-container {
    padding: 12px;
}
.section-title {
    font-size: 10px;
    font-weight: bold;
    color: #9c9a92;
    margin-bottom: 8px;
    margin-top: 12px;
}
.status-bar {
    padding: 8px 12px;
    border-radius: 8px;
    margin-bottom: 8px;
}
.status-green { background-color: rgba(118,185,0,0.15); }
.status-yellow { background-color: rgba(245,158,11,0.15); }
.status-red { background-color: rgba(239,68,68,0.15); }
.status-gray { background-color: rgba(107,114,128,0.15); }
.status-label { font-size: 11px; font-weight: bold; }
.green { color: #76b900; }
.yellow { color: #f59e0b; }
.red { color: #ef4444; }
.blue { color: #00b0f0; }
.muted { color: #9c9a92; }
.svc-card {
    background-color: #2a2a27;
    border-radius: 8px;
    padding: 10px 12px;
    margin-bottom: 6px;
}
.svc-name { font-size: 11px; font-weight: bold; }
.svc-detail { font-size: 9px; color: #9c9a92; }
.svc-badge {
    font-size: 9px; font-weight: bold;
    padding: 2px 8px; border-radius: 10px;
}
.badge-active { background-color: rgba(118,185,0,0.15); color: #76b900; }
.badge-inactive { background-color: rgba(239,68,68,0.15); color: #ef4444; }
.badge-rocm { background-color: rgba(0,176,240,0.15); color: #00b0f0; }
.badge-axum { background-color: rgba(156,154,146,0.15); color: #9c9a92; }
.badge-ok { background-color: rgba(118,185,0,0.15); color: #76b900; }
.badge-missing { background-color: rgba(239,68,68,0.15); color: #ef4444; }
.badge-mode-active { background-color: rgba(118,185,0,0.25); color: #76b900; font-weight: bold; }
.badge-mode-inactive { background-color: rgba(107,114,128,0.1); color: #9c9a92; }
.res-card {
    background-color: #2a2a27;
    border-radius: 8px;
    padding: 8px 12px;
    margin-bottom: 6px;
}
.res-label { font-size: 10px; color: #9c9a92; }
.res-val { font-size: 12px; font-weight: bold; color: #e8e7e0; }
.conn-status { font-size: 9px; color: #9c9a92; }
.restart-btn { background-color: rgba(249,115,22,0.15); color: #f97316; border-radius: 8px; padding: 6px 10px; font-size: 10px; font-weight: bold; }
.ssh-btn { background-color: rgba(0,176,240,0.15); color: #00b0f0; border-radius: 8px; padding: 6px 10px; font-size: 10px; font-weight: bold; }
.help-card { background-color: #2a2a27; border-radius: 8px; padding: 10px 12px; margin-bottom: 6px; }
.help-title { font-size: 11px; font-weight: bold; color: #f97316; margin-bottom: 4px; }
.help-text { font-size: 9px; color: #9c9a92; font-family: monospace; }
.tab-label { font-size: 10px; font-weight: bold; }
.config-label { font-size: 10px; color: #9c9a92; margin-bottom: 2px; }
.action-btn {
    background-color: rgba(118,185,0,0.15);
    color: #76b900;
    border-radius: 8px;
    padding: 8px 16px;
    font-size: 11px;
    font-weight: bold;
}
.action-btn-blue {
    background-color: rgba(0,176,240,0.15);
    color: #00b0f0;
    border-radius: 8px;
    padding: 8px 16px;
    font-size: 11px;
    font-weight: bold;
}
.model-card {
    background-color: #2a2a27;
    border-radius: 8px;
    padding: 8px 12px;
    margin-bottom: 4px;
}
.model-name { font-size: 11px; font-weight: bold; }
.model-detail { font-size: 9px; color: #9c9a92; }
entry {
    background-color: #1e1e1c;
    color: #e8e7e0;
    border-radius: 4px;
    padding: 4px 8px;
    font-size: 11px;
    min-height: 28px;
}
notebook header { background-color: #1e1e1c; }
notebook tab { padding: 4px 12px; color: #9c9a92; font-size: 10px; }
notebook tab:checked { color: #76b900; border-bottom: 2px solid #76b900; }
levelbar trough { min-height: 3px; border-radius: 2px; background-color: #1e1e1c; }
levelbar block.filled { border-radius: 2px; background-color: #76b900; min-height: 3px; }
separator { background-color: #444440; min-height: 1px; margin-top: 8px; margin-bottom: 8px; }
"#;

struct SvcInfo {
    name: &'static str,
    port: u16,
    default_detail: &'static str,
    backend: &'static str,
    badge_class: &'static str,
}

const SERVICES: &[SvcInfo] = &[
    SvcInfo { name: "ollama",       port: 11434, default_detail: "LLM Inference",    backend: "ROCm",  badge_class: "badge-rocm" },
    SvcInfo { name: "evo-metrics",  port: 8084,  default_detail: "System-Metriken",  backend: "Axum",  badge_class: "badge-axum" },
    SvcInfo { name: "evo-webhook",  port: 9000,  default_detail: "GitHub Webhook",   backend: "Axum",  badge_class: "badge-axum" },
];

pub fn build_popup() -> Window {
    let provider = CssProvider::new();
    provider.load_from_data(CSS.as_bytes()).expect("CSS load");
    if let Some(screen) = gtk::gdk::Screen::default() {
        StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let window = Window::new(WindowType::Toplevel);
    window.set_title("EVO-X2 Manager");
    window.set_default_size(400, 520);
    window.set_resizable(false);
    window.set_type_hint(gtk::gdk::WindowTypeHint::Dialog);
    window.set_skip_taskbar_hint(true);

    window.connect_delete_event(|w, _| {
        w.hide();
        glib::Propagation::Stop
    });

    let container = GtkBox::new(Orientation::Vertical, 0);
    container.style_context().add_class("popup-container");
    let loading = Label::new(Some("Verbinde mit EVO-X2..."));
    loading.style_context().add_class("muted");
    container.pack_start(&loading, false, false, 0);
    window.add(&container);
    window
}

pub fn update_popup(window: &Window, state: &WidgetState) {
    let saved_tab = ACTIVE_TAB.with(|t| t.get());

    if let Some(child) = window.children().first() {
        window.remove(child);
    }

    let outer = GtkBox::new(Orientation::Vertical, 0);
    outer.style_context().add_class("popup-container");

    // ── Status bar ──
    let status_box = GtkBox::new(Orientation::Horizontal, 8);
    let color = state.warning_color();
    status_box.style_context().add_class("status-bar");
    status_box.style_context().add_class(&format!("status-{color}"));

    let dot = Label::new(Some(match color {
        "green" => "\u{25CF}",
        "yellow" => "\u{26A0}",
        "red" => "\u{26D4}",
        _ => "\u{25CB}",
    }));
    dot.style_context().add_class(color);
    status_box.pack_start(&dot, false, false, 0);

    let (active, total) = state.active_count();
    let status_text = if total > 0 {
        format!("{active}/{total} Services aktiv")
    } else {
        match &state.connection {
            ConnectionState::Connected => "Verbunden".into(),
            ConnectionState::Connecting => "Verbinde...".into(),
            ConnectionState::Reconnecting(n) => format!("Reconnect #{n}"),
            ConnectionState::Error(e) => e.clone(),
        }
    };
    let status_lbl = Label::new(Some(&status_text));
    status_lbl.style_context().add_class("status-label");
    status_box.pack_start(&status_lbl, false, false, 0);

    if let Some(ref m) = state.metrics {
        let gtt_text = format!("GTT: {:.0}/{:.0} GB", m.gtt.used_gb, m.gtt.total_gb);
        let gtt_lbl = Label::new(Some(&gtt_text));
        gtt_lbl.style_context().add_class("muted");
        gtt_lbl.set_halign(Align::End);
        gtt_lbl.set_hexpand(true);
        status_box.pack_start(&gtt_lbl, true, true, 0);
    }

    outer.pack_start(&status_box, false, false, 0);

    // ── Notebook (4 Tabs) ──
    let notebook = Notebook::new();
    notebook.set_tab_pos(gtk::PositionType::Top);

    // Tab 1: Services (dynamisch)
    let tab1 = build_tab_services(state);
    notebook.append_page(&tab1, Some(&make_tab_label("network-server", "Services")));

    // Tab 2: Ressourcen (dynamisch)
    let tab2 = build_tab_resources(state);
    notebook.append_page(&tab2, Some(&make_tab_label("utilities-system-monitor", "Ressourcen")));

    // Tab 3: Ollama-Modelle (dynamisch)
    let tab3 = build_tab_ollama(state);
    notebook.append_page(&tab3, Some(&make_tab_label("applications-other", "Modelle")));

    // Tab 4: Config (lazy-loaded)
    if saved_tab == 3 {
        let tab4 = build_tab_config();
        notebook.append_page(&tab4, Some(&make_tab_label("preferences-system", "Config")));
    } else {
        let placeholder = GtkBox::new(Orientation::Vertical, 0);
        let lbl = Label::new(Some("Tab anklicken zum Laden..."));
        lbl.style_context().add_class("muted");
        lbl.set_margin_top(20);
        placeholder.pack_start(&lbl, false, false, 0);
        let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.add(&placeholder);
        notebook.append_page(&scroll, Some(&make_tab_label("preferences-system", "Config")));
    }

    notebook.set_vexpand(true);
    // Wichtig: Erst current_page setzen, DANN Signal verbinden.
    // append_page triggert intern switch_page mit Seite 0 — das darf
    // ACTIVE_TAB nicht überschreiben.
    notebook.set_current_page(Some(saved_tab));
    let restored_tab = saved_tab;
    // Signal blockiert kurz: erst nach show_all aktiv, damit GTK
    // keine spurious switch_page Events den gespeicherten Tab killen.
    let inhibit = std::rc::Rc::new(std::cell::Cell::new(true));
    let inhibit_clone = inhibit.clone();
    notebook.connect_switch_page(move |_, _, page_num| {
        if !inhibit_clone.get() {
            ACTIVE_TAB.with(|t| t.set(page_num));
        }
    });
    // Nochmal sicherstellen nach Signal-Verbindung
    notebook.set_current_page(Some(restored_tab));

    outer.pack_start(&notebook, true, true, 0);

    // ── Footer ──
    let footer = GtkBox::new(Orientation::Horizontal, 8);
    footer.set_margin_top(8);

    let title = Label::new(Some("EVO-X2"));
    title.style_context().add_class("status-label");
    title.style_context().add_class("green");
    footer.pack_start(&title, false, false, 0);

    // Ollama Restart-Button (HTTP-Call an evo-metrics /restart-ollama)
    let restart_btn = Button::with_label("\u{21BB} Ollama Restart");
    restart_btn.style_context().add_class("restart-btn");
    restart_btn.set_tooltip_text(Some("Ollama auf der EVO X2 neustarten (loest blockierte Modelle)"));
    let evo_ip = state.config.as_ref().map(|c| c.evo_ip.clone()).unwrap_or_default();
    let metrics_port = state.config.as_ref().map(|c| c.metrics_port).unwrap_or(8084);
    restart_btn.connect_clicked(move |btn| {
        btn.set_sensitive(false);
        btn.set_label("\u{231B} ...");
        let ip = evo_ip.clone();
        let port = metrics_port;
        // Ergebnis ueber Channel zurueck an GTK-Thread
        let (result_tx, result_rx) = glib::MainContext::channel::<bool>(glib::Priority::DEFAULT);
        std::thread::spawn(move || {
            let url = format!("http://{}:{}/restart-ollama", ip, port);
            let ok = std::process::Command::new("curl")
                .args(["-s", "--max-time", "20", "-X", "POST", &url])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            let _ = result_tx.send(ok);
        });
        let btn_clone = btn.clone();
        result_rx.attach(None, move |ok| {
            if ok {
                btn_clone.set_label("\u{2713} OK");
            } else {
                btn_clone.set_label("\u{2715} Fehler");
            }
            btn_clone.set_sensitive(true);
            let btn_reset = btn_clone.clone();
            glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                btn_reset.set_label("\u{21BB} Ollama Restart");
            });
            glib::ControlFlow::Break
        });
    });
    footer.pack_start(&restart_btn, false, false, 0);

    // SSH-Terminal-Button
    let ssh_ip = state.config.as_ref().map(|c| {
        if c.tailscale_ip.is_empty() { c.evo_ip.clone() } else { c.tailscale_ip.clone() }
    }).unwrap_or_default();
    let ssh_user = state.config.as_ref().map(|c| c.ssh_user.clone()).unwrap_or_else(|| "janpow".into());
    let ssh_btn = Button::with_label("\u{1F4BB} SSH");
    ssh_btn.style_context().add_class("ssh-btn");
    ssh_btn.set_tooltip_text(Some(&format!("Terminal: ssh {ssh_user}@{ssh_ip}")));
    ssh_btn.connect_clicked(move |_| {
        let target = format!("{}@{}", ssh_user, ssh_ip);
        // Versuche verschiedene Terminal-Emulatoren
        let terminals = [
            ("gnome-terminal", vec!["--", "ssh", "-t", &target]),
            ("xterm", vec!["-e", "ssh", "-t", &target]),
            ("konsole", vec!["-e", "ssh", "-t", &target]),
        ];
        for (term, args) in &terminals {
            if std::process::Command::new(term)
                .args(args)
                .spawn()
                .is_ok()
            {
                return;
            }
        }
        tracing::warn!("Kein Terminal-Emulator gefunden");
    });
    footer.pack_start(&ssh_btn, false, false, 0);

    let conn_text = match &state.connection {
        ConnectionState::Connected => "\u{25CF} Verbunden".to_string(),
        ConnectionState::Connecting => "\u{25CB} Verbinde...".to_string(),
        ConnectionState::Reconnecting(n) => format!("\u{21BB} Reconnect #{n}"),
        ConnectionState::Error(e) => format!("\u{2715} {e}"),
    };
    let conn_label = Label::new(Some(&conn_text));
    conn_label.style_context().add_class("conn-status");
    conn_label.set_halign(Align::End);
    conn_label.set_hexpand(true);
    footer.pack_start(&conn_label, true, true, 0);

    outer.pack_start(&footer, false, false, 0);

    window.add(&outer);
    window.show_all();
    // Jetzt Signal freigeben — GTK hat alle spurious switch_page Events gefeuert
    inhibit.set(false);
}

// ── Tab 1: Services ──

fn build_tab_services(state: &WidgetState) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(8);

    for svc in SERVICES {
        let status = state.metrics.as_ref()
            .and_then(|m| m.services.get(svc.name))
            .map(|s| s.as_str())
            .unwrap_or("unknown");
        let is_active = status == "active";

        let card = GtkBox::new(Orientation::Vertical, 4);
        card.style_context().add_class("svc-card");

        let header = GtkBox::new(Orientation::Horizontal, 6);

        let dot = Label::new(Some("\u{25CF}"));
        dot.style_context().add_class(if is_active { "green" } else { "red" });
        header.pack_start(&dot, false, false, 0);

        let name = Label::new(Some(svc.name));
        name.style_context().add_class("svc-name");
        name.set_hexpand(true);
        name.set_halign(Align::Start);
        header.pack_start(&name, true, true, 0);

        let port_lbl = Label::new(Some(&format!(":{}", svc.port)));
        port_lbl.style_context().add_class("muted");
        header.pack_start(&port_lbl, false, false, 0);

        let backend = Label::new(Some(svc.backend));
        backend.style_context().add_class("svc-badge");
        backend.style_context().add_class(svc.badge_class);
        header.pack_start(&backend, false, false, 0);

        let status_badge = Label::new(Some(if is_active { "aktiv" } else { status }));
        status_badge.style_context().add_class("svc-badge");
        status_badge.style_context().add_class(if is_active { "badge-active" } else { "badge-inactive" });
        header.pack_start(&status_badge, false, false, 0);

        card.pack_start(&header, false, false, 0);

        // Ollama: Zeige dynamisch geladene Modelle
        let detail_text = if svc.name == "ollama" {
            state.metrics.as_ref()
                .and_then(|m| m.ollama.as_ref())
                .map(|o| {
                    if o.running_models.is_empty() {
                        "Keine Modelle geladen".to_string()
                    } else {
                        o.running_models.iter()
                            .map(|m| {
                                let short = m.name.split(':').next().unwrap_or(&m.name);
                                format!("{short} ({:.0} GB, {})", m.size_gb, m.processor)
                            })
                            .collect::<Vec<_>>()
                            .join(" + ")
                    }
                })
                .unwrap_or_else(|| svc.default_detail.to_string())
        } else {
            svc.default_detail.to_string()
        };

        let detail = Label::new(Some(&detail_text));
        detail.style_context().add_class("svc-detail");
        detail.set_halign(Align::Start);
        detail.set_line_wrap(true);
        card.pack_start(&detail, false, false, 0);

        content.pack_start(&card, false, false, 0);
    }

    content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);
    let (active, total) = state.active_count();
    let summary = Label::new(Some(&format!("{active} von {total} Services aktiv")));
    summary.style_context().add_class("muted");
    content.pack_start(&summary, false, false, 0);

    let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scroll.add(&content);
    scroll
}

// ── Tab 2: Resources ──

fn build_tab_resources(state: &WidgetState) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(8);

    if let Some(ref m) = state.metrics {
        // GTT
        content.pack_start(&build_resource_bar("GTT Speicher", m.gtt.used_gb, m.gtt.total_gb, "GB",
            if m.gtt.total_bytes > 0 { m.gtt.used_bytes as f64 / m.gtt.total_bytes as f64 } else { 0.0 }), false, false, 0);

        // RAM
        content.pack_start(&build_resource_bar("RAM", m.ram.used_gb, m.ram.total_gb, "GB",
            if m.ram.total_bytes > 0 { m.ram.used_bytes as f64 / m.ram.total_bytes as f64 } else { 0.0 }), false, false, 0);

        // GPU Temperatur
        if let Some(temp) = m.gpu.temperature_c {
            let card = GtkBox::new(Orientation::Horizontal, 8);
            card.style_context().add_class("res-card");
            let label = Label::new(Some("GPU Temperatur"));
            label.style_context().add_class("res-label");
            card.pack_start(&label, false, false, 0);
            let val = Label::new(Some(&format!("{temp} \u{00B0}C")));
            val.style_context().add_class("res-val");
            val.style_context().add_class(if temp > 85 { "red" } else if temp > 70 { "yellow" } else { "green" });
            val.set_halign(Align::End);
            val.set_hexpand(true);
            card.pack_start(&val, true, true, 0);
            content.pack_start(&card, false, false, 0);
        }

        // GPU Auslastung
        if let Some(util) = m.gpu.utilization_pct {
            content.pack_start(&build_resource_bar("GPU Auslastung", util as f64, 100.0, "%",
                util as f64 / 100.0), false, false, 0);
        }

        // CPU Load
        let cpu_card = GtkBox::new(Orientation::Vertical, 2);
        cpu_card.style_context().add_class("res-card");
        let cpu_header = GtkBox::new(Orientation::Horizontal, 8);
        let cpu_label = Label::new(Some("CPU Load"));
        cpu_label.style_context().add_class("res-label");
        cpu_header.pack_start(&cpu_label, false, false, 0);
        let load_text = format!("{:.1} / {:.1} / {:.1}", m.cpu_load.min1, m.cpu_load.min5, m.cpu_load.min15);
        let load_val = Label::new(Some(&load_text));
        load_val.style_context().add_class("res-val");
        load_val.style_context().add_class(if m.cpu_load.min1 > 8.0 { "red" } else if m.cpu_load.min1 > 4.0 { "yellow" } else { "green" });
        load_val.set_halign(Align::End);
        load_val.set_hexpand(true);
        cpu_header.pack_start(&load_val, true, true, 0);
        cpu_card.pack_start(&cpu_header, false, false, 0);
        content.pack_start(&cpu_card, false, false, 0);

        // Disk
        for disk in &m.disks {
            let pct = if disk.total_gb > 0.0 { disk.used_gb / disk.total_gb } else { 0.0 };
            let label = format!("Disk {}", disk.mount);
            content.pack_start(&build_resource_bar(&label, disk.used_gb, disk.total_gb, "GB", pct), false, false, 0);
        }

        content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);

        // System-Info (dynamisch aus Metrics)
        let info_title = Label::new(Some("SYSTEM"));
        info_title.style_context().add_class("section-title");
        info_title.set_halign(Align::Start);
        content.pack_start(&info_title, false, false, 0);

        let sys = &m.system;
        let mut sys_rows: Vec<(&str, String)> = Vec::new();
        if !sys.soc.is_empty() { sys_rows.push(("SoC", sys.soc.clone())); }
        if !sys.gpu_arch.is_empty() { sys_rows.push(("GPU", sys.gpu_arch.clone())); }
        if !sys.ram_spec.is_empty() { sys_rows.push(("RAM", sys.ram_spec.clone())); }
        if sys.cpu_cores > 0 { sys_rows.push(("Kerne", format!("{}", sys.cpu_cores))); }
        if sys.uptime_seconds > 0 {
            let h = sys.uptime_seconds / 3600;
            let min = (sys.uptime_seconds % 3600) / 60;
            sys_rows.push(("Uptime", format!("{h}h {min}m")));
        }

        // Tailscale-IP
        if let Some(ref ts) = m.tailscale {
            if let Some(ref ip) = ts.ip {
                sys_rows.push(("Tailscale", ip.clone()));
            }
        }

        for (l, v) in &sys_rows {
            let row = GtkBox::new(Orientation::Horizontal, 8);
            let lbl = Label::new(Some(l)); lbl.style_context().add_class("res-label"); row.pack_start(&lbl, false, false, 0);
            let val = Label::new(Some(v)); val.style_context().add_class("res-val"); val.set_halign(Align::End); val.set_hexpand(true);
            row.pack_start(&val, true, true, 0);
            content.pack_start(&row, false, false, 0);
        }
    } else {
        let p = Label::new(Some("Keine Metriken verfuegbar"));
        p.style_context().add_class("muted"); p.set_margin_top(20);
        content.pack_start(&p, false, false, 0);
    }

    let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scroll.add(&content);
    scroll
}

// ── Tab 3: Ollama-Modelle ──

fn build_tab_ollama(state: &WidgetState) -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(8);

    let ollama = state.metrics.as_ref().and_then(|m| m.ollama.as_ref());

    match ollama {
        Some(info) => {
            // Laufende Modelle
            let title = Label::new(Some("LAUFENDE MODELLE"));
            title.style_context().add_class("section-title");
            title.set_halign(Align::Start);
            content.pack_start(&title, false, false, 0);

            if info.running_models.is_empty() {
                let lbl = Label::new(Some("Keine Modelle geladen"));
                lbl.style_context().add_class("muted");
                lbl.set_halign(Align::Start);
                content.pack_start(&lbl, false, false, 0);
            } else {
                for model in &info.running_models {
                    let card = GtkBox::new(Orientation::Vertical, 2);
                    card.style_context().add_class("model-card");

                    let header = GtkBox::new(Orientation::Horizontal, 6);
                    let dot = Label::new(Some("\u{25CF}"));
                    dot.style_context().add_class("green");
                    header.pack_start(&dot, false, false, 0);

                    let name = Label::new(Some(&model.name));
                    name.style_context().add_class("model-name");
                    name.set_hexpand(true);
                    name.set_halign(Align::Start);
                    name.set_line_wrap(true);
                    header.pack_start(&name, true, true, 0);

                    card.pack_start(&header, false, false, 0);

                    let detail = format!(
                        "{:.1} GB total \u{2022} {:.1} GB VRAM \u{2022} {}",
                        model.size_gb, model.vram_gb, model.processor
                    );
                    let detail_lbl = Label::new(Some(&detail));
                    detail_lbl.style_context().add_class("model-detail");
                    detail_lbl.set_halign(Align::Start);
                    card.pack_start(&detail_lbl, false, false, 0);

                    content.pack_start(&card, false, false, 0);
                }
            }

            content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);

            // Verfuegbare Modelle
            let avail_title = Label::new(Some("VERFUEGBARE MODELLE"));
            avail_title.style_context().add_class("section-title");
            avail_title.set_halign(Align::Start);
            content.pack_start(&avail_title, false, false, 0);

            let running_names: Vec<&str> = info.running_models.iter().map(|m| m.name.as_str()).collect();
            let idle: Vec<&str> = info.available_models.iter()
                .filter(|n| !running_names.contains(&n.as_str()))
                .map(|s| s.as_str())
                .collect();

            if idle.is_empty() && info.available_models.is_empty() {
                let lbl = Label::new(Some("Keine Modelle installiert"));
                lbl.style_context().add_class("muted");
                content.pack_start(&lbl, false, false, 0);
            } else if idle.is_empty() {
                let lbl = Label::new(Some("Alle Modelle aktiv"));
                lbl.style_context().add_class("green");
                content.pack_start(&lbl, false, false, 0);
            } else {
                for name in &idle {
                    let row = GtkBox::new(Orientation::Horizontal, 6);
                    row.style_context().add_class("model-card");
                    let dot = Label::new(Some("\u{25CB}"));
                    dot.style_context().add_class("muted");
                    row.pack_start(&dot, false, false, 0);
                    let lbl = Label::new(Some(name));
                    lbl.style_context().add_class("svc-detail");
                    lbl.set_halign(Align::Start);
                    row.pack_start(&lbl, true, true, 0);
                    content.pack_start(&row, false, false, 0);
                }
            }

            // Zusammenfassung
            content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);
            let summary = format!(
                "{} laufend, {} installiert",
                info.running_models.len(),
                info.available_models.len()
            );
            let summary_lbl = Label::new(Some(&summary));
            summary_lbl.style_context().add_class("muted");
            content.pack_start(&summary_lbl, false, false, 0);
        }
        None => {
            let lbl = Label::new(Some("Ollama nicht erreichbar"));
            lbl.style_context().add_class("muted");
            lbl.set_margin_top(20);
            content.pack_start(&lbl, false, false, 0);
        }
    }

    let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scroll.add(&content);
    scroll
}

// ── Tab 4: Config ──

fn build_tab_config() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(8);

    let config = EvoConfig::load();

    let title = Label::new(Some("EVO-X2 VERBINDUNG"));
    title.style_context().add_class("section-title");
    title.set_halign(Align::Start);
    content.pack_start(&title, false, false, 0);

    // LAN IP
    let (ip_entry, _) = add_config_row(&content, "LAN IP", &config.evo_ip, "192.168.x.x");

    // Tailscale IP
    let (ts_entry, _) = add_config_row(&content, "Tailscale IP", &config.tailscale_ip, "100.x.x.x");

    // Netzwerk-Modus (LAN / Tailscale)
    let mode_label = Label::new(Some("Netzwerk-Modus"));
    mode_label.style_context().add_class("config-label");
    mode_label.set_halign(Align::Start);
    content.pack_start(&mode_label, false, false, 0);

    let mode_box = GtkBox::new(Orientation::Horizontal, 4);
    let btn_lan = Button::with_label("LAN");
    let btn_ts = Button::with_label("Tailscale");

    if config.network_mode == "tailscale" {
        btn_ts.style_context().add_class("badge-mode-active");
        btn_lan.style_context().add_class("badge-mode-inactive");
    } else {
        btn_lan.style_context().add_class("badge-mode-active");
        btn_ts.style_context().add_class("badge-mode-inactive");
    }

    let mode_state: Rc<Cell<bool>> = Rc::new(Cell::new(config.network_mode == "tailscale"));

    let ms1 = Rc::clone(&mode_state);
    let bl1 = btn_lan.clone();
    let bt1 = btn_ts.clone();
    btn_lan.connect_clicked(move |_| {
        ms1.set(false);
        bl1.style_context().remove_class("badge-mode-inactive");
        bl1.style_context().add_class("badge-mode-active");
        bt1.style_context().remove_class("badge-mode-active");
        bt1.style_context().add_class("badge-mode-inactive");
    });

    let ms2 = Rc::clone(&mode_state);
    let bl2 = btn_lan.clone();
    let bt2 = btn_ts.clone();
    btn_ts.connect_clicked(move |_| {
        ms2.set(true);
        bt2.style_context().remove_class("badge-mode-inactive");
        bt2.style_context().add_class("badge-mode-active");
        bl2.style_context().remove_class("badge-mode-active");
        bl2.style_context().add_class("badge-mode-inactive");
    });

    mode_box.pack_start(&btn_lan, true, true, 0);
    mode_box.pack_start(&btn_ts, true, true, 0);
    content.pack_start(&mode_box, false, false, 0);

    // SSH User
    let (user_entry, _) = add_config_row(&content, "SSH Benutzer", &config.ssh_user, "jan");

    // Metrics Port
    let (port_entry, _) = add_config_row(&content, "Metrics Port", &config.metrics_port.to_string(), "8084");

    content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);

    let gh_title = Label::new(Some("GITHUB INTEGRATION"));
    gh_title.style_context().add_class("section-title");
    gh_title.set_halign(Align::Start);
    content.pack_start(&gh_title, false, false, 0);

    // GitHub URL
    let (gh_entry, _) = add_config_row(&content, "GitHub Repo URL", &config.github_url, "https://github.com/user/repo");

    // Setup Dir
    let (setup_entry, _) = add_config_row(&content, "Setup-Verzeichnis", &config.setup_dir, "~/Projekte/evo/setup");

    content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);

    // Save button
    let btn_box = GtkBox::new(Orientation::Horizontal, 8);
    let save_btn = Button::with_label("\u{1F4BE} Speichern");
    save_btn.style_context().add_class("action-btn");

    let status_label = Label::new(None);
    status_label.style_context().add_class("muted");

    let status_clone = status_label.clone();
    save_btn.connect_clicked(move |_| {
        let ip_text = ip_entry.text().to_string().trim().to_string();
        let ts_text = ts_entry.text().to_string().trim().to_string();
        let port_text = port_entry.text().to_string().trim().to_string();
        let user_text = user_entry.text().to_string().trim().to_string();

        // IP-Validierung
        for (label, val) in &[("LAN-IP", &ip_text), ("Tailscale-IP", &ts_text)] {
            if !val.is_empty() {
                let parts: Vec<&str> = val.split('.').collect();
                let valid_ipv4 = parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok());
                let valid_hostname = val.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-');
                if !valid_ipv4 && !valid_hostname {
                    status_clone.set_text(&format!("\u{274C} Ungueltige {label}!"));
                    return;
                }
            }
        }

        let port: u16 = match port_text.parse() {
            Ok(p) if p > 0 => p,
            _ => {
                status_clone.set_text("\u{274C} Ungueltiger Port (1-65535)!");
                return;
            }
        };

        if user_text.is_empty() {
            status_clone.set_text("\u{274C} SSH-Benutzer darf nicht leer sein!");
            return;
        }

        let new_config = EvoConfig {
            evo_ip: ip_text,
            tailscale_ip: ts_text,
            network_mode: if mode_state.get() { "tailscale".into() } else { "lan".into() },
            ssh_user: user_text,
            metrics_port: port,
            github_url: gh_entry.text().to_string().trim().to_string(),
            setup_dir: setup_entry.text().to_string().trim().to_string(),
            poll_interval_secs: EvoConfig::load().poll_interval_secs,
        };
        match new_config.save() {
            Ok(_) => status_clone.set_text("\u{2705} Gespeichert!"),
            Err(e) => status_clone.set_text(&format!("\u{274C} Fehler: {e}")),
        }
    });

    btn_box.pack_start(&save_btn, false, false, 0);
    btn_box.pack_start(&status_label, false, false, 0);
    content.pack_start(&btn_box, false, false, 0);

    let path_info = Label::new(Some(&format!("Config: {}", EvoConfig::config_path().display())));
    path_info.style_context().add_class("muted");
    path_info.set_margin_top(8);
    path_info.set_halign(Align::Start);
    content.pack_start(&path_info, false, false, 0);

    // ── Notfall-Hilfe ──
    content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);

    let help_title = Label::new(Some("NOTFALL-VERBINDUNG"));
    help_title.style_context().add_class("section-title");
    help_title.set_halign(Align::Start);
    content.pack_start(&help_title, false, false, 0);

    let help_card = GtkBox::new(Orientation::Vertical, 4);
    help_card.style_context().add_class("help-card");

    let help_header = Label::new(Some("Wenn nichts mehr geht:"));
    help_header.style_context().add_class("help-title");
    help_header.set_halign(Align::Start);
    help_card.pack_start(&help_header, false, false, 0);

    let ssh_target = format!(
        "ssh -t {}@{}", config.ssh_user,
        if config.tailscale_ip.is_empty() { &config.evo_ip } else { &config.tailscale_ip }
    );
    let commands: Vec<(&str, String)> = vec![
        ("1. SSH Terminal oeffnen", ssh_target),
        ("2. Ollama neustarten", "sudo systemctl restart ollama".into()),
        ("3. Ollama Status pruefen", "systemctl status ollama".into()),
        ("4. Laufende Modelle", "curl -s localhost:11434/api/ps | python3 -m json.tool".into()),
        ("5. Alle Modelle entladen", "curl -s localhost:11434/api/generate -d '{\"model\":\"qwen3:32b\",\"keep_alive\":0}'".into()),
        ("6. System neustarten", "sudo reboot".into()),
    ];

    for (label, cmd) in &commands {
        let row = GtkBox::new(Orientation::Vertical, 1);
        let lbl = Label::new(Some(label));
        lbl.style_context().add_class("gpu-stat-val");
        lbl.set_halign(Align::Start);
        row.pack_start(&lbl, false, false, 0);

        let cmd_lbl = Label::new(Some(cmd));
        cmd_lbl.style_context().add_class("help-text");
        cmd_lbl.set_halign(Align::Start);
        cmd_lbl.set_selectable(true);
        row.pack_start(&cmd_lbl, false, false, 0);

        help_card.pack_start(&row, false, false, 2);
    }

    content.pack_start(&help_card, false, false, 0);

    let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scroll.add(&content);
    scroll
}

// ── Helpers ──

fn make_tab_label(icon_name: &str, text: &str) -> GtkBox {
    let hbox = GtkBox::new(Orientation::Horizontal, 4);
    let icon = Image::from_icon_name(Some(icon_name), gtk::IconSize::SmallToolbar);
    hbox.pack_start(&icon, false, false, 0);
    let label = Label::new(Some(text));
    label.style_context().add_class("tab-label");
    hbox.pack_start(&label, false, false, 0);
    hbox.show_all();
    hbox
}

fn add_config_row(container: &GtkBox, label_text: &str, value: &str, placeholder: &str) -> (Entry, Label) {
    let label = Label::new(Some(label_text));
    label.style_context().add_class("config-label");
    label.set_halign(Align::Start);
    container.pack_start(&label, false, false, 0);

    let entry = Entry::new();
    entry.set_text(value);
    entry.set_placeholder_text(Some(placeholder));
    container.pack_start(&entry, false, false, 0);

    (entry, label)
}

fn build_resource_bar(name: &str, used: f64, total: f64, unit: &str, pct: f64) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 2);
    card.style_context().add_class("res-card");
    let header = GtkBox::new(Orientation::Horizontal, 8);
    let label = Label::new(Some(name)); label.style_context().add_class("res-label");
    header.pack_start(&label, false, false, 0);
    let val_text = format!("{used:.1} / {total:.1} {unit}");
    let val = Label::new(Some(&val_text)); val.style_context().add_class("res-val");
    if pct > 0.9 { val.style_context().add_class("red"); }
    else if pct > 0.7 { val.style_context().add_class("yellow"); }
    val.set_halign(Align::End); val.set_hexpand(true);
    header.pack_start(&val, true, true, 0);
    card.pack_start(&header, false, false, 0);
    let bar = LevelBar::for_interval(0.0, 1.0); bar.set_value(pct.clamp(0.0, 1.0)); bar.set_margin_top(2);
    card.pack_start(&bar, false, false, 0);
    card
}

