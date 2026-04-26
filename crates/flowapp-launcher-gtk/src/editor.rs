use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::Result;
use gtk::prelude::*;
use tracing::{info, warn};

use crate::apps::{self, App};

const MODES: &[&str] = &["app", "tab"];

pub fn show(parent: Option<&gtk::Window>, apps_yml: PathBuf, on_saved: impl Fn() + 'static) {
    let initial = match apps::load(&apps_yml) {
        Ok(a) => a,
        Err(e) => {
            warn!("apps.yml laden fehlgeschlagen: {e}");
            Vec::new()
        }
    };

    let model: Rc<RefCell<Vec<App>>> = Rc::new(RefCell::new(initial));

    let dialog = gtk::Dialog::with_buttons(
        Some("FlowAudit Apps bearbeiten"),
        parent,
        gtk::DialogFlags::MODAL,
        &[
            ("Abbrechen", gtk::ResponseType::Cancel),
            ("Speichern", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_default_size(900, 600);
    dialog.set_position(gtk::WindowPosition::Center);

    let outer = dialog.content_area();
    outer.set_spacing(6);
    outer.set_margin_start(8);
    outer.set_margin_end(8);
    outer.set_margin_top(8);
    outer.set_margin_bottom(8);

    let header = gtk::Label::new(Some(
        "Hinzufuegen, Bearbeiten, Sortieren. „Speichern\" schreibt apps.yml zurueck.",
    ));
    header.set_halign(gtk::Align::Start);
    outer.pack_start(&header, false, false, 0);

    let scroll = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scroll.set_vexpand(true);
    let listbox = gtk::ListBox::new();
    listbox.set_selection_mode(gtk::SelectionMode::None);
    scroll.add(&listbox);
    outer.pack_start(&scroll, true, true, 0);

    let listbox_rc = Rc::new(listbox);
    rebuild_rows(&listbox_rc, &model);

    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let add_btn = gtk::Button::with_label("+ App hinzufuegen");
    let model_add = Rc::clone(&model);
    let listbox_add = Rc::clone(&listbox_rc);
    add_btn.connect_clicked(move |_| {
        model_add.borrow_mut().push(App {
            name: "neue_app".into(),
            url: "http://localhost:8080".into(),
            mode: "app".into(),
            comment: Some("".into()),
            icon: None,
        });
        rebuild_rows(&listbox_add, &model_add);
    });
    toolbar.pack_start(&add_btn, false, false, 0);
    outer.pack_start(&toolbar, false, false, 0);

    dialog.show_all();

    let model_save = Rc::clone(&model);
    let path_save = apps_yml.clone();
    dialog.connect_response(move |dlg, response| {
        if response == gtk::ResponseType::Accept {
            let apps = model_save.borrow().clone();
            match apps::write(&path_save, &apps) {
                Ok(()) => {
                    info!("apps.yml geschrieben ({} Eintraege)", apps.len());
                    on_saved();
                }
                Err(e) => {
                    let err = gtk::MessageDialog::new(
                        Some(dlg),
                        gtk::DialogFlags::MODAL,
                        gtk::MessageType::Error,
                        gtk::ButtonsType::Ok,
                        &format!("Speichern fehlgeschlagen:\n{e}"),
                    );
                    err.run();
                    err.close();
                    return;
                }
            }
        }
        dlg.close();
    });
}

fn rebuild_rows(listbox: &Rc<gtk::ListBox>, model: &Rc<RefCell<Vec<App>>>) {
    for child in listbox.children() {
        listbox.remove(&child);
    }

    let count = model.borrow().len();
    for idx in 0..count {
        let row = build_row(idx, model, listbox);
        listbox.add(&row);
    }
    listbox.show_all();
}

fn build_row(
    idx: usize,
    model: &Rc<RefCell<Vec<App>>>,
    listbox: &Rc<gtk::ListBox>,
) -> gtk::ListBoxRow {
    let app = model.borrow()[idx].clone();
    let row = gtk::ListBoxRow::new();
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    hbox.set_margin_start(4);
    hbox.set_margin_end(4);
    hbox.set_margin_top(2);
    hbox.set_margin_bottom(2);

    let icon_entry = gtk::Entry::new();
    icon_entry.set_width_chars(3);
    icon_entry.set_max_length(8);
    icon_entry.set_placeholder_text(Some("Icon"));
    icon_entry.set_text(app.icon.as_deref().unwrap_or(""));
    icon_entry.set_tooltip_text(Some("Emoji als Icon (leer = automatisches Default)"));
    let model_icon = Rc::clone(model);
    icon_entry.connect_changed(move |e| {
        let val = e.text().to_string();
        let mut m = model_icon.borrow_mut();
        if let Some(a) = m.get_mut(idx) {
            a.icon = if val.trim().is_empty() { None } else { Some(val) };
        }
    });
    hbox.pack_start(&icon_entry, false, false, 0);

    let name_entry = gtk::Entry::new();
    name_entry.set_width_chars(18);
    name_entry.set_placeholder_text(Some("Name"));
    name_entry.set_text(&app.name);
    let model_name = Rc::clone(model);
    name_entry.connect_changed(move |e| {
        if let Some(a) = model_name.borrow_mut().get_mut(idx) {
            a.name = e.text().to_string();
        }
    });
    hbox.pack_start(&name_entry, false, false, 0);

    let url_entry = gtk::Entry::new();
    url_entry.set_placeholder_text(Some("URL"));
    url_entry.set_text(&app.url);
    let model_url = Rc::clone(model);
    url_entry.connect_changed(move |e| {
        if let Some(a) = model_url.borrow_mut().get_mut(idx) {
            a.url = e.text().to_string();
        }
    });
    hbox.pack_start(&url_entry, true, true, 0);

    let mode_combo = gtk::ComboBoxText::new();
    for m in MODES {
        mode_combo.append_text(m);
    }
    let active_mode = MODES.iter().position(|m| *m == app.mode).unwrap_or(0);
    mode_combo.set_active(Some(active_mode as u32));
    mode_combo.set_tooltip_text(Some("app = eigenes Chrome-Fenster, tab = Browser-Tab"));
    let model_mode = Rc::clone(model);
    mode_combo.connect_changed(move |c| {
        if let Some(text) = c.active_text() {
            if let Some(a) = model_mode.borrow_mut().get_mut(idx) {
                a.mode = text.to_string();
            }
        }
    });
    hbox.pack_start(&mode_combo, false, false, 0);

    let comment_entry = gtk::Entry::new();
    comment_entry.set_placeholder_text(Some("Kommentar / Tooltip"));
    comment_entry.set_width_chars(24);
    comment_entry.set_text(app.comment.as_deref().unwrap_or(""));
    let model_comment = Rc::clone(model);
    comment_entry.connect_changed(move |e| {
        let val = e.text().to_string();
        if let Some(a) = model_comment.borrow_mut().get_mut(idx) {
            a.comment = if val.is_empty() { None } else { Some(val) };
        }
    });
    hbox.pack_start(&comment_entry, true, true, 0);

    let up_btn = gtk::Button::with_label("\u{2B06}");
    up_btn.set_tooltip_text(Some("Nach oben"));
    up_btn.set_sensitive(idx > 0);
    let model_up = Rc::clone(model);
    let listbox_up = Rc::clone(listbox);
    up_btn.connect_clicked(move |_| {
        {
            let mut m = model_up.borrow_mut();
            if idx > 0 && idx < m.len() {
                m.swap(idx - 1, idx);
            }
        }
        rebuild_rows(&listbox_up, &model_up);
    });
    hbox.pack_start(&up_btn, false, false, 0);

    let down_btn = gtk::Button::with_label("\u{2B07}");
    down_btn.set_tooltip_text(Some("Nach unten"));
    down_btn.set_sensitive(idx + 1 < model.borrow().len());
    let model_down = Rc::clone(model);
    let listbox_down = Rc::clone(listbox);
    down_btn.connect_clicked(move |_| {
        {
            let mut m = model_down.borrow_mut();
            if idx + 1 < m.len() {
                m.swap(idx, idx + 1);
            }
        }
        rebuild_rows(&listbox_down, &model_down);
    });
    hbox.pack_start(&down_btn, false, false, 0);

    let del_btn = gtk::Button::with_label("\u{1F5D1}");
    del_btn.set_tooltip_text(Some("Loeschen"));
    let model_del = Rc::clone(model);
    let listbox_del = Rc::clone(listbox);
    del_btn.connect_clicked(move |_| {
        {
            let mut m = model_del.borrow_mut();
            if idx < m.len() {
                m.remove(idx);
            }
        }
        rebuild_rows(&listbox_del, &model_del);
    });
    hbox.pack_start(&del_btn, false, false, 0);

    row.add(&hbox);
    row
}

#[allow(dead_code)]
fn _check(_: Result<()>) {}
