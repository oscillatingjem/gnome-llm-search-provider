//! `agent-search`: the prompt box (a spotlight-style window) and the settings window.
//! D-Bus-activatable; stays running for [prompt] idle_minutes after use so it reopens instantly.

mod prefs;
mod prompt;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use agent_search_core::config::{self, Config};
use agent_search_core::Identity;
use agent_search_platform as platform;
use libadwaita as adw;

const CSS: &str = "
.prompt-box { padding: 12px; }
.prompt-entry { font-size: 1.35em; min-height: 44px; }
.prompt-rows row { padding: 6px 4px; }
";

/// Shared state for the app's windows.
pub struct State {
    pub app: adw::Application,
    pub config: Config,
    pub gateway: platform::Gateway,
    pub signature: Vec<Option<(i128, u64)>>,
    pub prompt: Option<prompt::PromptWindow>,
    pub settings: Option<adw::Window>,
    idle_source: Option<glib::SourceId>,
    _hold: Option<gio::ApplicationHoldGuard>,
}

pub type Shared = Rc<RefCell<State>>;

impl State {
    pub fn reload(&mut self) {
        match config::load(None) {
            Ok(c) => self.config = c,
            Err(e) => platform::log(&format!("agents.toml: {e}")),
        }
        self.gateway = platform::Gateway::default();
        self.signature = config::signature(None);
    }
}

pub fn open(state: &Shared, text: Option<String>) {
    cancel_idle_timer(state);
    {
        let mut s = state.borrow_mut();
        if config::signature(None) != s.signature {
            s.reload();
        }
    }
    let existing = state.borrow().prompt.clone();
    let window = match existing {
        Some(w) => w,
        None => {
            let w = prompt::PromptWindow::new(state);
            state.borrow_mut().prompt = Some(w.clone());
            w
        }
    };
    window.summon(text);
}

pub fn open_settings(state: &Shared) {
    cancel_idle_timer(state);
    // Never call into GTK while holding a borrow: hiding the window re-enters us (focus-out → dismiss).
    let prompt = state.borrow().prompt.clone();
    if let Some(w) = prompt {
        w.window.set_visible(false);
    }
    let existing = state.borrow().settings.clone().filter(|w| w.is_visible());
    let window = existing.unwrap_or_else(|| {
        let w = prefs::build(state);
        state.borrow_mut().settings = Some(w.clone());
        w
    });
    window.present();
}

pub fn cancel_idle_timer(state: &Shared) {
    if let Some(src) = state.borrow_mut().idle_source.take() {
        src.remove();
    }
}

pub fn start_idle_timer(state: &Shared) {
    cancel_idle_timer(state);
    let app = state.borrow().app.clone();
    if app.windows().iter().any(|w| w.is_visible()) {
        return; // e.g. the settings window is still open
    }
    let minutes = state.borrow().config.prompt_idle_minutes;
    if minutes <= 0.0 {
        gio::Settings::sync();
        app.quit();
        return;
    }
    let weak = Rc::downgrade(state);
    let src = glib::timeout_add_seconds_local_once((minutes * 60.0) as u32, move || {
        if let Some(state) = weak.upgrade() {
            state.borrow_mut().idle_source = None;
            gio::Settings::sync();
            let app = state.borrow().app.clone();
            app.quit();
        }
    });
    state.borrow_mut().idle_source = Some(src);
}

fn main() -> glib::ExitCode {
    let id = Identity::current();
    let app = adw::Application::builder().application_id(id.app_id.as_str()).build();
    let config = config::load(None).unwrap_or_else(|e| {
        platform::log(&format!("agents.toml: {e}"));
        Config::default()
    });
    let state: Shared = Rc::new(RefCell::new(State {
        app: app.clone(),
        config,
        gateway: platform::Gateway::default(),
        signature: config::signature(None),
        prompt: None,
        settings: None,
        idle_source: None,
        _hold: None,
    }));

    let search = gio::SimpleAction::new("search", Some(glib::VariantTy::STRING));
    let s = state.clone();
    search.connect_activate(move |_, text| open(&s, text.and_then(|t| t.get::<String>())));
    app.add_action(&search);
    let preferences = gio::SimpleAction::new("preferences", None);
    let s = state.clone();
    preferences.connect_activate(move |_, _| open_settings(&s));
    app.add_action(&preferences);

    let s = state.clone();
    app.connect_startup(move |app| {
        let css = gtk::CssProvider::new();
        css.load_from_string(CSS);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        }
        // Stay running while hidden, until the idle timer quits.
        s.borrow_mut()._hold = Some(app.hold());
    });
    let s = state.clone();
    app.connect_activate(move |_| open(&s, None));
    app.run()
}

use gtk4 as gtk;
