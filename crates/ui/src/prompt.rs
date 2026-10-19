//! The prompt box: a spotlight-style window for sending a prompt straight to an agent.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use agent_search_core::{catalog, launch, query, ranker};
use agent_search_platform as platform;
use gtk4 as gtk;
use libadwaita as adw;

use crate::Shared;

#[derive(Clone)]
pub struct PromptWindow {
    pub window: adw::ApplicationWindow,
    entry: gtk::Entry,
    rows: gtk::ListBox,
    hint: gtk::Label,
    ids: Rc<RefCell<Vec<String>>>,
    state: std::rc::Weak<RefCell<crate::State>>,
}

impl PromptWindow {
    pub fn new(state: &Shared) -> PromptWindow {
        let app = state.borrow().app.clone();
        let name = agent_search_core::Identity::current().name;
        let window = adw::ApplicationWindow::builder()
            .application(&app)
            .title(name.as_str())
            .resizable(false)
            .default_width(680)
            .hide_on_close(true)
            .build();

        let entry = gtk::Entry::builder()
            .placeholder_text("Ask anything…  @model to choose · !high for effort")
            .hexpand(true)
            .build();
        entry.add_css_class("prompt-entry");
        entry.set_icon_from_icon_name(gtk::EntryIconPosition::Primary, Some("system-search-symbolic"));

        let rows = gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Browse).build();
        rows.add_css_class("boxed-list");
        rows.add_css_class("prompt-rows");

        let hint = gtk::Label::builder()
            .label("Enter to send · ↑↓ to choose · Esc to close · Ctrl+, for settings")
            .margin_top(4)
            .build();
        hint.add_css_class("dim-label");
        hint.add_css_class("caption");

        let gear = gtk::Button::builder()
            .icon_name("emblem-system-symbolic")
            .tooltip_text("Settings (Ctrl+,)")
            .valign(gtk::Align::Center)
            .build();
        gear.add_css_class("flat");
        gear.add_css_class("circular");

        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top.append(&entry);
        top.append(&gear);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
        content.add_css_class("prompt-box");
        content.append(&top);
        content.append(&rows);
        content.append(&hint);
        window.set_content(Some(&content));

        let pw = PromptWindow {
            window: window.clone(),
            entry: entry.clone(),
            rows: rows.clone(),
            hint,
            ids: Rc::new(RefCell::new(Vec::new())),
            state: Rc::downgrade(state),
        };

        let p = pw.clone();
        entry.connect_changed(move |_| p.refresh());
        let p = pw.clone();
        entry.connect_activate(move |_| p.run_selected());
        let p = pw.clone();
        rows.connect_row_activated(move |_, row| p.run(row.index()));
        let s = state.clone();
        gear.connect_clicked(move |_| crate::open_settings(&s));

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let p = pw.clone();
        keys.connect_key_pressed(move |_, key, _, mods| p.on_key(key, mods));
        window.add_controller(keys);

        // Like a spotlight: clicking elsewhere closes it.
        let p = pw.clone();
        window.connect_is_active_notify(move |w| {
            if !w.is_active() {
                p.dismiss();
            }
        });
        pw.refresh();
        pw
    }

    fn refresh(&self) {
        let Some(state) = self.state.upgrade() else { return };
        while let Some(child) = self.rows.first_child() {
            self.rows.remove(&child);
        }
        let terms: Vec<String> = agent_search_core::pystr::split(&self.entry.text()).into_iter().map(str::to_string).collect();
        let q = query::parse(&terms);
        let found = {
            let mut s = state.borrow_mut();
            let cfg = s.config.clone();
            let models = s.gateway.models(&cfg);
            let cands = catalog::candidates(&cfg, &models, &launch::which);
            ranker::rank(&q, &cands, &cfg).unwrap_or_else(|e| {
                platform::log(&format!("rank failed: {e}"));
                Vec::new()
            })
        };
        let mut ids = Vec::new();
        for r in &found {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&r.title()).as_str())
                .subtitle(glib::markup_escape_text(&r.subtitle()).as_str())
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::builder().icon_name(r.candidate.icon.as_str()).pixel_size(32).build());
            self.rows.append(&row);
            ids.push(r.result_id());
        }
        *self.ids.borrow_mut() = ids;
        self.rows.set_visible(!found.is_empty());
        self.hint.set_visible(!found.is_empty());
        if let Some(first) = self.rows.row_at_index(0) {
            self.rows.select_row(Some(&first));
        }
    }

    fn run_selected(&self) {
        if let Some(row) = self.rows.selected_row() {
            self.run(row.index());
        }
    }

    fn run(&self, index: i32) {
        let Some(state) = self.state.upgrade() else { return };
        let Some(rid) = usize::try_from(index).ok().and_then(|i| self.ids.borrow().get(i).cloned()) else { return };
        let terms: Vec<String> = agent_search_core::pystr::split(&self.entry.text()).into_iter().map(str::to_string).collect();
        let prompt = query::parse(&terms).prompt;
        let result = {
            let s = state.borrow();
            launch::activation(&s.config, &rid, &prompt, &launch::which)
        };
        match result.and_then(|a| platform::execute(&a)) {
            Ok(()) => {}
            Err(e) => platform::log(&format!("ActivateResult failed: {e}")),
        }
        self.entry.set_text("");
        self.dismiss();
    }

    fn move_selection(&self, step: i32) {
        let current = self.rows.selected_row().map_or(-1, |r| r.index());
        if let Some(target) = self.rows.row_at_index((current + step).max(0)) {
            self.rows.select_row(Some(&target));
        }
    }

    fn on_key(&self, key: gtk::gdk::Key, mods: gtk::gdk::ModifierType) -> glib::Propagation {
        use gtk::gdk::Key;
        if key == Key::Escape {
            self.dismiss();
            return glib::Propagation::Stop;
        }
        if key == Key::comma && mods.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
            if let Some(state) = self.state.upgrade() {
                crate::open_settings(&state);
            }
            return glib::Propagation::Stop;
        }
        if key == Key::Down || key == Key::Tab {
            self.move_selection(1);
            return glib::Propagation::Stop;
        }
        if key == Key::Up || key == Key::ISO_Left_Tab {
            self.move_selection(-1);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    }

    pub fn summon(&self, text: Option<String>) {
        if let Some(t) = text {
            self.entry.set_text(&t);
        }
        self.refresh();
        self.window.present();
        self.entry.grab_focus();
        self.entry.set_position(-1);
    }

    pub fn dismiss(&self) {
        self.window.set_visible(false);
        if let Some(state) = self.state.upgrade() {
            crate::start_idle_timer(&state);
        }
    }
}
