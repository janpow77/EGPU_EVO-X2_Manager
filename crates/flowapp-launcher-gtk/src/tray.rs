use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Context, Result};
use gtk::prelude::*;
use libappindicator::{AppIndicator, AppIndicatorStatus};
use tracing::info;

use crate::apps::{self, App, icon_for};
use crate::editor;
use crate::launcher;

const ICON_NAME: &str = "flowapp-launcher";

pub struct Tray {
    indicator: Rc<RefCell<AppIndicator>>,
    apps_yml: Rc<PathBuf>,
}

fn install_icon(source_dir: &std::path::Path) -> Option<PathBuf> {
    let source = source_dir.join("icon.png");
    if !source.exists() {
        return None;
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".local/share")
        });
    let icon_dir = base.join("icons").join("flowapp-launcher");
    std::fs::create_dir_all(&icon_dir).ok()?;
    let target = icon_dir.join(format!("{ICON_NAME}.png"));
    std::fs::copy(&source, &target).ok()?;
    Some(icon_dir)
}

pub fn create(apps_yml: PathBuf) -> Result<Tray> {
    let apps_dir = apps_yml
        .parent()
        .context("apps.yml ohne Parent")?
        .to_path_buf();

    let icon_dir = install_icon(&apps_dir);

    let mut indicator = AppIndicator::new("FlowAudit Apps", ICON_NAME);
    if let Some(dir) = &icon_dir {
        indicator.set_icon_theme_path(&dir.to_string_lossy());
        indicator.set_icon_full(ICON_NAME, "FlowAudit Apps");
    } else {
        indicator.set_icon_full("applications-internet", "FlowAudit Apps");
    }
    indicator.set_status(AppIndicatorStatus::Active);
    indicator.set_title("FlowAudit Apps");

    let indicator_rc = Rc::new(RefCell::new(indicator));
    let apps_yml_rc = Rc::new(apps_yml);

    refresh_menu(&indicator_rc, &apps_yml_rc);

    Ok(Tray {
        indicator: indicator_rc,
        apps_yml: apps_yml_rc,
    })
}

pub fn refresh_menu(indicator: &Rc<RefCell<AppIndicator>>, apps_yml: &Rc<PathBuf>) {
    let mut menu = match build_menu(indicator, apps_yml) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Menü-Build fehlgeschlagen: {e}");
            return;
        }
    };
    indicator.borrow_mut().set_menu(&mut menu);
    let count = apps::load(apps_yml).map(|a| a.len()).unwrap_or(0);
    info!("Menü aktualisiert ({count} Apps)");
}

impl Tray {
    pub fn refresh(&self) {
        refresh_menu(&self.indicator, &self.apps_yml);
    }
}

fn build_menu(
    indicator: &Rc<RefCell<AppIndicator>>,
    apps_yml: &Rc<PathBuf>,
) -> Result<gtk::Menu> {
    let menu = gtk::Menu::new();
    let apps_list = apps::load(apps_yml)?;

    let pwa: Vec<&App> = apps_list
        .iter()
        .filter(|a| a.mode == "app" && a.name != "FlowAudit_Apps")
        .collect();
    let tab: Vec<&App> = apps_list.iter().filter(|a| a.mode == "tab").collect();

    if !pwa.is_empty() {
        append_header(&menu, "— Apps —");
        for app in &pwa {
            append_app(&menu, app);
        }
    }

    if !tab.is_empty() {
        menu.append(&gtk::SeparatorMenuItem::new());
        append_header(&menu, "— Browser-Tools —");
        for app in &tab {
            append_app(&menu, app);
        }
    }

    menu.append(&gtk::SeparatorMenuItem::new());

    let edit_item = gtk::MenuItem::with_label("⚙️  Apps bearbeiten...");
    let indicator_edit = Rc::clone(indicator);
    let apps_yml_edit = Rc::clone(apps_yml);
    edit_item.connect_activate(move |_| {
        let indicator_save = Rc::clone(&indicator_edit);
        let apps_yml_save = Rc::clone(&apps_yml_edit);
        editor::show(None, (*apps_yml_edit).clone(), move || {
            refresh_menu(&indicator_save, &apps_yml_save);
        });
    });
    menu.append(&edit_item);

    let web_item = gtk::MenuItem::with_label("🌐  Launcher im Browser oeffnen");
    web_item.connect_activate(|_| launcher::open_launcher_web());
    menu.append(&web_item);

    menu.append(&gtk::SeparatorMenuItem::new());

    let quit_item = gtk::MenuItem::with_label("Beenden");
    quit_item.connect_activate(|_| gtk::main_quit());
    menu.append(&quit_item);

    menu.show_all();
    Ok(menu)
}

fn append_header(menu: &gtk::Menu, label: &str) {
    let item = gtk::MenuItem::with_label(label);
    item.set_sensitive(false);
    menu.append(&item);
}

fn append_app(menu: &gtk::Menu, app: &App) {
    let label = format!("{}  {}", icon_for(app), app.name);
    let item = gtk::MenuItem::with_label(&label);
    let tooltip = app.comment.clone().unwrap_or_else(|| app.url.clone());
    item.set_tooltip_text(Some(&tooltip));
    let app_clone = app.clone();
    item.connect_activate(move |_| launcher::launch(&app_clone));
    menu.append(&item);
}
