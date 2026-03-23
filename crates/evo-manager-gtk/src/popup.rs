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
.badge-vulkan { background-color: rgba(118,185,0,0.15); color: #76b900; }
.badge-rocm { background-color: rgba(0,176,240,0.15); color: #00b0f0; }
.badge-ok { background-color: rgba(118,185,0,0.15); color: #76b900; }
.badge-missing { background-color: rgba(239,68,68,0.15); color: #ef4444; }
.res-card {
    background-color: #2a2a27;
    border-radius: 8px;
    padding: 8px 12px;
    margin-bottom: 6px;
}
.res-label { font-size: 10px; color: #9c9a92; }
.res-val { font-size: 12px; font-weight: bold; color: #e8e7e0; }
.conn-status { font-size: 9px; color: #9c9a92; }
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
    model: &'static str,
    backend: &'static str,
    badge_class: &'static str,
}

const SERVICES: &[SvcInfo] = &[
    SvcInfo { name: "llama-creative",  port: 8080, model: "Mistral Small 3.1 24B", backend: "Vulkan", badge_class: "badge-vulkan" },
    SvcInfo { name: "llama-code",      port: 8081, model: "Qwen3-32B",             backend: "ROCm",   badge_class: "badge-rocm" },
    SvcInfo { name: "llama-embedding", port: 8082, model: "Nomic-Embed v1.5",      backend: "ROCm",   badge_class: "badge-rocm" },
    SvcInfo { name: "llama-ocr",       port: 8083, model: "Docling",               backend: "ROCm",   badge_class: "badge-rocm" },
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

    // Nur die dynamischen Tabs (Services, Ressourcen) neubauen.
    // Setup und Config sind statisch und werden nur einmal gebaut.
    // Die Status-Bar wird immer aktualisiert.
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

    // Tab 1+2: Dynamisch (werden bei jedem Update neu gebaut)
    let tab1 = build_tab_services(state);
    notebook.append_page(&tab1, Some(&make_tab_label("network-server", "Services")));

    let tab2 = build_tab_resources(state);
    notebook.append_page(&tab2, Some(&make_tab_label("utilities-system-monitor", "Ressourcen")));

    // Tab 3+4: Statisch — nur neubauen wenn sichtbar (vermeidet Dateisystem-Zugriffe)
    if saved_tab == 2 {
        let tab3 = build_tab_setup();
        notebook.append_page(&tab3, Some(&make_tab_label("drive-harddisk", "Setup")));
    } else {
        let placeholder = GtkBox::new(Orientation::Vertical, 0);
        let lbl = Label::new(Some("Tab anklicken zum Laden..."));
        lbl.style_context().add_class("muted");
        lbl.set_margin_top(20);
        placeholder.pack_start(&lbl, false, false, 0);
        let scroll = ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        scroll.add(&placeholder);
        notebook.append_page(&scroll, Some(&make_tab_label("drive-harddisk", "Setup")));
    }

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
    notebook.set_current_page(Some(saved_tab));
    notebook.connect_switch_page(|_, _, page_num| {
        ACTIVE_TAB.with(|t| t.set(page_num));
    });

    outer.pack_start(&notebook, true, true, 0);

    // ── Footer ──
    let footer = GtkBox::new(Orientation::Horizontal, 8);
    footer.set_margin_top(8);

    let title = Label::new(Some("EVO-X2"));
    title.style_context().add_class("status-label");
    title.style_context().add_class("green");
    footer.pack_start(&title, false, false, 0);

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

        let detail = Label::new(Some(svc.model));
        detail.style_context().add_class("svc-detail");
        detail.set_halign(Align::Start);
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
        content.pack_start(&build_resource_bar("GTT Speicher", m.gtt.used_gb, m.gtt.total_gb, "GB",
            if m.gtt.total_bytes > 0 { m.gtt.used_bytes as f64 / m.gtt.total_bytes as f64 } else { 0.0 }), false, false, 0);
        content.pack_start(&build_resource_bar("RAM", m.ram.used_gb, m.ram.total_gb, "GB",
            if m.ram.total_bytes > 0 { m.ram.used_bytes as f64 / m.ram.total_bytes as f64 } else { 0.0 }), false, false, 0);

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

        content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);
        let info_title = Label::new(Some("SYSTEM"));
        info_title.style_context().add_class("section-title");
        info_title.set_halign(Align::Start);
        content.pack_start(&info_title, false, false, 0);
        for (l, v) in &[("SoC", "AMD Ryzen AI Max+ 395"), ("GPU", "RDNA 3.5, 40 CUs"), ("RAM", "128 GB LPDDR5X")] {
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

// ── Tab 3: Setup (USB-Erstellung) ──

fn build_tab_setup() -> ScrolledWindow {
    let content = GtkBox::new(Orientation::Vertical, 4);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_margin_top(8);

    let config = EvoConfig::load();
    let setup_dir = std::path::PathBuf::from(&config.setup_dir);

    // Checklist
    let title = Label::new(Some("USB-PAKET STATUS"));
    title.style_context().add_class("section-title");
    title.set_halign(Align::Start);
    content.pack_start(&title, false, false, 0);

    let checks = [
        ("Secrets (.env)", setup_dir.join("secrets/.env").exists()),
        ("SSH-Keys", setup_dir.join("config/authorized_keys").exists()),
        ("Ubuntu Autoinstall", setup_dir.join("autoinstall/user-data").exists()),
        ("Ubuntu ISO", setup_dir.join("iso").is_dir() && std::fs::read_dir(setup_dir.join("iso")).map(|d| d.count() > 0).unwrap_or(false)),
        ("Kernel-Pakete", setup_dir.join("kernel").is_dir() && std::fs::read_dir(setup_dir.join("kernel")).map(|d| d.count() > 0).unwrap_or(false)),
        ("amdgpu Firmware", setup_dir.join("firmware").is_dir() && std::fs::read_dir(setup_dir.join("firmware")).map(|d| d.count() > 0).unwrap_or(false)),
        ("Container-Images", setup_dir.join("container/llama-vulkan-amdvlk.tar").exists() && setup_dir.join("container/llama-rocm.tar").exists()),
    ];

    let all_ok = checks.iter().all(|(_, ok)| *ok);

    for (name, ok) in &checks {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.style_context().add_class("svc-card");

        let icon = Label::new(Some(if *ok { "\u{2705}" } else { "\u{274C}" }));
        row.pack_start(&icon, false, false, 0);

        let lbl = Label::new(Some(name));
        lbl.style_context().add_class("svc-name");
        lbl.set_hexpand(true);
        lbl.set_halign(Align::Start);
        row.pack_start(&lbl, true, true, 0);

        let badge = Label::new(Some(if *ok { "OK" } else { "fehlt" }));
        badge.style_context().add_class("svc-badge");
        badge.style_context().add_class(if *ok { "badge-ok" } else { "badge-missing" });
        row.pack_start(&badge, false, false, 0);

        content.pack_start(&row, false, false, 0);
    }

    if all_ok {
        let ready = Label::new(Some("\u{2705} USB-Paket bereit!"));
        ready.style_context().add_class("green");
        ready.set_margin_top(8);
        content.pack_start(&ready, false, false, 0);
    }

    content.pack_start(&Separator::new(Orientation::Horizontal), false, false, 0);

    // Action buttons
    let actions_title = Label::new(Some("AKTIONEN"));
    actions_title.style_context().add_class("section-title");
    actions_title.set_halign(Align::Start);
    content.pack_start(&actions_title, false, false, 0);

    let btn_row1 = GtkBox::new(Orientation::Horizontal, 8);

    let btn_secrets = Button::with_label("\u{1F511} Secrets-Wizard");
    btn_secrets.style_context().add_class("action-btn");
    let sd1 = config.setup_dir.clone();
    btn_secrets.connect_clicked(move |_| {
        spawn_terminal(&format!("cd '{}' && bash prepare-secrets.sh", sd1));
    });
    btn_row1.pack_start(&btn_secrets, true, true, 0);

    let btn_usb = Button::with_label("\u{1F4BE} USB-Paket bauen");
    btn_usb.style_context().add_class("action-btn-blue");
    let sd2 = config.setup_dir.clone();
    btn_usb.connect_clicked(move |_| {
        spawn_terminal(&format!("cd '{}' && bash prepare-usb.sh", sd2));
    });
    btn_row1.pack_start(&btn_usb, true, true, 0);

    content.pack_start(&btn_row1, false, false, 0);

    // Info
    let info = Label::new(Some("Buttons oeffnen ein Terminal-Fenster fuer die jeweilige Aktion."));
    info.style_context().add_class("muted");
    info.set_margin_top(8);
    info.set_line_wrap(true);
    content.pack_start(&info, false, false, 0);

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

    // IP
    let (ip_entry, _) = add_config_row(&content, "EVO-X2 IP (LAN)", &config.evo_ip, "192.168.x.x");

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
    let (gh_entry, _) = add_config_row(&content, "GitHub Repo URL", &config.github_url, "https://github.com/user/evo-x2-config");

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
        // Validierung
        let ip_text = ip_entry.text().to_string().trim().to_string();
        let port_text = port_entry.text().to_string().trim().to_string();
        let user_text = user_entry.text().to_string().trim().to_string();

        // IP-Validierung: leer (erlaubt) oder gueltige IPv4
        if !ip_text.is_empty() {
            let parts: Vec<&str> = ip_text.split('.').collect();
            let valid_ipv4 = parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok());
            let valid_hostname = ip_text.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-');
            if !valid_ipv4 && !valid_hostname {
                status_clone.set_text("\u{274C} Ungueltige IP-Adresse!");
                return;
            }
        }

        // Port-Validierung
        let port: u16 = match port_text.parse() {
            Ok(p) if p > 0 => p,
            _ => {
                status_clone.set_text("\u{274C} Ungueltiger Port (1-65535)!");
                return;
            }
        };

        // SSH-User darf nicht leer sein
        if user_text.is_empty() {
            status_clone.set_text("\u{274C} SSH-Benutzer darf nicht leer sein!");
            return;
        }

        let new_config = EvoConfig {
            evo_ip: ip_text,
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

    // Config path info
    let path_info = Label::new(Some(&format!("Config: {}", EvoConfig::config_path().display())));
    path_info.style_context().add_class("muted");
    path_info.set_margin_top(8);
    path_info.set_halign(Align::Start);
    content.pack_start(&path_info, false, false, 0);

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
    let bar = LevelBar::for_interval(0.0, 1.0); bar.set_value(pct); bar.set_margin_top(2);
    card.pack_start(&bar, false, false, 0);
    card
}

fn spawn_terminal(cmd: &str) {
    let full_cmd = format!("{cmd}; echo; echo 'Druecke Enter zum Schliessen...'; read");
    // Try common terminal emulators
    for terminal in &["gnome-terminal", "xfce4-terminal", "konsole", "xterm"] {
        let args = match *terminal {
            "gnome-terminal" => vec!["--".to_string(), "bash".into(), "-c".into(), full_cmd.clone()],
            "konsole" => vec!["-e".to_string(), "bash".into(), "-c".into(), full_cmd.clone()],
            _ => vec!["-e".to_string(), format!("bash -c '{}'", full_cmd.replace('\'', "'\\''"))],
        };
        if std::process::Command::new(terminal)
            .args(&args)
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    eprintln!("Kein Terminal-Emulator gefunden!");
}
