//! The settings window, opened from the gear on the prompt box.
//!
//! Every change is written straight into agents.toml, the single source of settings, keeping its
//! comments. The window refreshes itself if agents.toml is edited while it's open.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use agent_search_core::config::{self, App};
use agent_search_core::edit::{Edit, Scalar};
use agent_search_core::launch;
use agent_search_platform as platform;
use agent_search_platform::shortcut;
use gtk4 as gtk;
use libadwaita as adw;

use crate::Shared;

const DOCS: &str = "https://github.com/oscillatingjem/gnome-llm-search-provider/blob/main/docs/";

struct Ctx {
    state: Shared,
    window: adw::Window,
    stack: adw::ViewStack,
    toasts: adw::ToastOverlay,
    shortcut_label: RefCell<Option<gtk::ShortcutLabel>>,
    status_row: RefCell<Option<adw::ActionRow>>,
    refresh_source: RefCell<Option<glib::SourceId>>,
    _monitor: RefCell<Option<gio::FileMonitor>>,
}

type C = Rc<Ctx>;

pub fn build(state: &Shared) -> adw::Window {
    let app = state.borrow().app.clone();
    let name = agent_search_core::Identity::current().name;
    let window = adw::Window::builder()
        .application(&app)
        .title(format!("{name} Settings").as_str())
        .default_width(600)
        .default_height(680)
        .build();
    let stack = adw::ViewStack::new();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&stack));
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build()));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&toasts));
    window.set_content(Some(&view));

    let ctx: C = Rc::new(Ctx {
        state: state.clone(),
        window: window.clone(),
        stack,
        toasts,
        shortcut_label: RefCell::new(None),
        status_row: RefCell::new(None),
        refresh_source: RefCell::new(None),
        _monitor: RefCell::new(None),
    });
    build_pages(&ctx);

    let keys = gtk::EventControllerKey::new();
    let w = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            w.close();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);

    // Stay in sync with agents.toml when it's edited elsewhere while open.
    let file = gio::File::for_path(directory(&ctx).join(config::AGENTS_FILE));
    if let Ok(monitor) = file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, None::<&gio::Cancellable>) {
        let weak = Rc::downgrade(&ctx);
        monitor.connect_changed(move |_, _, _, _| {
            if let Some(ctx) = weak.upgrade() {
                schedule_refresh(&ctx);
            }
        });
        *ctx._monitor.borrow_mut() = Some(monitor);
    }

    let keep = ctx.clone();
    window.connect_close_request(move |_| {
        if let Some(m) = keep._monitor.borrow_mut().take() {
            m.cancel();
        }
        // Once closed (not while closing, when it still counts as visible), let the app idle out.
        let state = keep.state.clone();
        glib::idle_add_local_once(move || crate::start_idle_timer(&state));
        glib::Propagation::Proceed
    });
    window
}

fn directory(ctx: &C) -> std::path::PathBuf {
    ctx.state.borrow().config.directory.clone().unwrap_or_else(config::config_dir)
}

fn build_pages(ctx: &C) {
    let visible = ctx.stack.visible_child_name();
    while let Some(child) = ctx.stack.first_child() {
        ctx.stack.remove(&child);
    }
    ctx.stack.add_titled_with_icon(&general_page(ctx), Some("general"), "General", "preferences-system-symbolic");
    ctx.stack.add_titled_with_icon(&agents_page(ctx), Some("agents"), "Agents", "system-users-symbolic");
    ctx.stack.add_titled_with_icon(&gateway_page(ctx), Some("gateway"), "Gateway", "network-server-symbolic");
    if let Some(name) = visible {
        ctx.stack.set_visible_child_name(&name);
    }
}

fn schedule_refresh(ctx: &C) {
    if ctx.refresh_source.borrow().is_some() {
        return;
    }
    let weak = Rc::downgrade(ctx);
    let src = glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
        let Some(ctx) = weak.upgrade() else { return };
        ctx.refresh_source.borrow_mut().take();
        let ours = config::signature(Some(&directory(&ctx))) == ctx.state.borrow().signature;
        if !ours {
            ctx.state.borrow_mut().reload();
            build_pages(&ctx);
            toast(&ctx, "Updated from agents.toml");
        }
    });
    *ctx.refresh_source.borrow_mut() = Some(src);
}

fn toast(ctx: &C, text: &str) {
    let t = adw::Toast::new(glib::markup_escape_text(text).as_str());
    t.set_timeout(3);
    ctx.toasts.add_toast(t);
}

fn write(ctx: &C, edits: Vec<Edit>) {
    let dir = directory(ctx);
    match config::update(&edits, Some(&dir)) {
        Ok(()) => ctx.state.borrow_mut().reload(),
        Err(e) => toast(ctx, &format!("Couldn't save: {e}")),
    }
}

fn save(ctx: &C, table: &str, key: &str, value: Scalar) {
    write(ctx, vec![Edit::table(table, key, value)]);
}

fn save_app(ctx: &C, app_id: &str, key: &str, value: Scalar) {
    write(ctx, vec![Edit::item("app", app_id, key, value)]);
}

// --- rows -----------------------------------------------------------------------

fn spin_row(title: &str, subtitle: &str, value: f64, lower: f64, upper: f64, step: f64, digits: u32, on_change: impl Fn(f64) + 'static) -> adw::SpinRow {
    let row = adw::SpinRow::with_range(lower, upper, step);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_digits(digits);
    row.set_value(value);
    row.connect_value_notify(move |r| on_change(r.value()));
    row
}

fn entry_row(title: &str, text: &str, password: bool, on_apply: impl Fn(String) + 'static) -> adw::EntryRow {
    let row: adw::EntryRow = if password { adw::PasswordEntryRow::new().upcast() } else { adw::EntryRow::new() };
    row.set_title(title);
    row.set_show_apply_button(true);
    row.set_text(text);
    row.connect_apply(move |r| on_apply(r.text().trim().to_string()));
    row
}

fn button_row(title: &str, subtitle: &str, label: &str, icon: Option<&str>, on_click: impl Fn() + 'static) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).subtitle(subtitle).build();
    let button = gtk::Button::builder().valign(gtk::Align::Center).tooltip_text(label).build();
    match icon {
        Some(icon) => {
            button.set_icon_name(icon);
            button.add_css_class("flat");
        }
        None => button.set_label(label),
    }
    button.connect_clicked(move |_| on_click());
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    row
}

fn open_file(ctx: &C, name: &str) {
    let uri = gio::File::for_path(directory(ctx).join(name)).uri();
    let _ = platform::open_uri(&uri);
}

// --- General ----------------------------------------------------------------------

fn general_page(ctx: &C) -> adw::PreferencesPage {
    let c = ctx.state.borrow().config.clone();
    let page = adw::PreferencesPage::new();

    let bx = adw::PreferencesGroup::builder().title("Prompt Box").build();
    let row = adw::ActionRow::builder().title("Keyboard Shortcut").subtitle("Opens the prompt box from anywhere").build();
    let label = gtk::ShortcutLabel::builder().valign(gtk::Align::Center).disabled_text("None").build();
    label.set_accelerator(&shortcut::get());
    let change = gtk::Button::builder().label("Change…").valign(gtk::Align::Center).build();
    let cc = ctx.clone();
    change.connect_clicked(move |_| shortcut_dialog(&cc));
    row.add_suffix(&label);
    row.add_suffix(&change);
    *ctx.shortcut_label.borrow_mut() = Some(label);
    bx.add(&row);
    let cc = ctx.clone();
    bx.add(&spin_row("Keep Running After Closing", "Minutes; opens instantly within this time. 0 quits at once",
                     c.prompt_idle_minutes, 0.0, 480.0, 5.0, 0, move |v| save(&cc, "prompt", "idle_minutes", Scalar::Int(v as i64))));
    page.add(&bx);

    let results = adw::PreferencesGroup::builder().title("Results").description("Used by both the prompt box and GNOME search").build();
    let cc = ctx.clone();
    results.add(&spin_row("Minimum Characters", "Shorter prompts show no rows", c.min_chars as f64, 1.0, 20.0, 1.0, 0,
                          move |v| save(&cc, "search", "min_chars", Scalar::Int(v as i64))));
    let cc = ctx.clone();
    results.add(&spin_row("Rows Shown", "GNOME search shows about five", c.max_rows as f64, 1.0, 10.0, 1.0, 0,
                          move |v| save(&cc, "search", "max_rows", Scalar::Int(v as i64))));
    let cc = ctx.clone();
    results.add(&spin_row("Low Effort Up To", "Prompts with this many words or fewer get low effort", c.low_effort_max_words as f64, 0.0, 50.0, 1.0, 0,
                          move |v| save(&cc, "routing", "low_effort_max_words", Scalar::Int(v as i64))));
    page.add(&results);

    let files = adw::PreferencesGroup::builder().title("Configuration Files").build();
    let cc = ctx.clone();
    files.add(&button_row("agents.toml", "Add agents, aliases and ranking rules. Every setting is explained in the file",
                          "Open", Some("document-edit-symbolic"), move || open_file(&cc, config::AGENTS_FILE)));
    let cc = ctx.clone();
    files.add(&button_row("Restore Defaults", "Puts the settings on this page back to factory values. Agents, rules, the gateway and the shortcut are kept",
                          "Restore…", None, move || confirm_restore(&cc)));
    files.add(&button_row("Documentation", "Search syntax, providers and troubleshooting", "Open", Some("help-browser-symbolic"),
                          || {
                              let _ = platform::open_uri(&format!("{DOCS}USAGE.md"));
                          }));
    page.add(&files);
    page
}

fn confirm_restore(ctx: &C) {
    let dialog = adw::AlertDialog::new(
        Some("Restore Defaults?"),
        Some("Minimum characters, rows shown, the low-effort limit and how long the box keeps running go back to factory values in agents.toml. Your agents, aliases, ranking rules, gateway and shortcut are kept."),
    );
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("restore", "Restore");
    dialog.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    let cc = ctx.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "restore" {
            restore(&cc);
        }
    });
    dialog.present(Some(&ctx.window));
}

fn restore(ctx: &C) {
    match config::restore_defaults(Some(&directory(ctx))) {
        Ok(()) => {
            ctx.state.borrow_mut().reload();
            ctx.window.close();
            crate::open_settings(&ctx.state);
            if let Some(w) = ctx.state.borrow().settings.clone() {
                let t = adw::Toast::new("Defaults restored");
                if let Some(overlay) = w.content().and_then(|v| v.downcast::<adw::ToolbarView>().ok()).and_then(|v| v.content()).and_then(|c| c.downcast::<adw::ToastOverlay>().ok()) {
                    overlay.add_toast(t);
                }
            }
        }
        Err(e) => toast(ctx, &format!("Couldn't restore defaults: {e}")),
    }
}

fn shortcut_dialog(ctx: &C) {
    let dialog = adw::Window::builder().transient_for(&ctx.window).modal(true).title("Set Shortcut").resizable(false).default_width(420).build();
    let status = adw::StatusPage::builder()
        .icon_name("input-keyboard-symbolic")
        .title("Press the New Shortcut")
        .description("Esc to cancel · Backspace to remove the shortcut")
        .build();
    status.add_css_class("compact");
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&status));
    dialog.set_content(Some(&view));
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let cc = ctx.clone();
    let d = dialog.clone();
    keys.connect_key_pressed(move |_, key, _, state| {
        use gtk::gdk::Key;
        let mods = state & gtk::accelerator_get_default_mod_mask();
        if key == Key::Escape && mods.is_empty() {
            d.close();
        } else if key == Key::BackSpace && mods.is_empty() {
            set_shortcut(&cc, "");
            d.close();
        } else if is_modifier(key) || mods.is_empty() {
            // Wait for the full combination; a shortcut needs at least one modifier.
        } else {
            let lower = key.to_lower();
            set_shortcut(&cc, &gtk::accelerator_name(lower, mods));
            d.close();
        }
        glib::Propagation::Stop
    });
    dialog.add_controller(keys);
    dialog.present();
}

fn is_modifier(key: gtk::gdk::Key) -> bool {
    use gtk::gdk::Key;
    [
        Key::Shift_L, Key::Shift_R, Key::Control_L, Key::Control_R, Key::Alt_L, Key::Alt_R, Key::Super_L, Key::Super_R,
        Key::Meta_L, Key::Meta_R, Key::Hyper_L, Key::Hyper_R, Key::ISO_Level3_Shift, Key::Caps_Lock,
    ]
    .contains(&key)
}

fn set_shortcut(ctx: &C, binding: &str) {
    if let Err(e) = shortcut::set(binding) {
        toast(ctx, &format!("Couldn't set the shortcut: {e}"));
    }
    if let Some(label) = ctx.shortcut_label.borrow().as_ref() {
        label.set_accelerator(&shortcut::get());
    }
}

// --- Agents -----------------------------------------------------------------------

fn agents_page(ctx: &C) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Agents")
        .description("Turn agents on or off, and change how they rank. Higher priority ranks higher.")
        .build();
    let apps = ctx.state.borrow().config.apps.clone();
    for app in &apps {
        group.add(&agent_row(ctx, app));
    }
    page.add(&group);
    let more = adw::PreferencesGroup::new();
    let cc = ctx.clone();
    more.add(&button_row("Add an Agent", "New agents are added in agents.toml, which has ready-made examples",
                         "Open", Some("list-add-symbolic"), move || open_file(&cc, config::AGENTS_FILE)));
    page.add(&more);
    page
}

fn agent_row(ctx: &C, app: &App) -> adw::ExpanderRow {
    let missing: Vec<&str> = app.requires.iter().map(String::as_str).filter(|p| launch::which(p).is_none()).collect();
    let subtitle = if !missing.is_empty() {
        format!("Not installed: {}", missing.join(", "))
    } else if !app.command.is_empty() {
        let words: Vec<&str> = app
            .command
            .iter()
            .filter_map(|a| match a {
                config::CommandItem::Arg(s) if !s.contains('{') => Some(s.as_str()),
                _ => None,
            })
            .collect();
        let s = format!("Runs {}", words.join(" "));
        s.chars().take(65).collect()
    } else {
        format!("Opens {}", app.url.split('?').next().unwrap_or(""))
    };
    let row = adw::ExpanderRow::builder()
        .title(glib::markup_escape_text(&app.name).as_str())
        .subtitle(glib::markup_escape_text(&subtitle).as_str())
        .show_enable_switch(true)
        .enable_expansion(app.enabled)
        .build();
    let (cc, id) = (ctx.clone(), app.id.clone());
    row.connect_enable_expansion_notify(move |r| save_app(&cc, &id, "enabled", Scalar::Bool(r.enables_expansion())));
    let (cc, id) = (ctx.clone(), app.id.clone());
    row.add_row(&spin_row("Priority", "Added to its ranking score", app.priority, -10.0, 10.0, 0.5, 1,
                          move |v| save_app(&cc, &id, "priority", Scalar::Float(v))));
    if !app.command.is_empty() {
        let (cc, id) = (ctx.clone(), app.id.clone());
        let folder = entry_row("Working Folder", &app.cwd, false, move |v| {
            save_app(&cc, &id, "cwd", Scalar::Str(if v.is_empty() { "~".into() } else { v }))
        });
        let pick = gtk::Button::builder().icon_name("folder-open-symbolic").valign(gtk::Align::Center).tooltip_text("Choose Folder").build();
        pick.add_css_class("flat");
        let (cc, id, name, f) = (ctx.clone(), app.id.clone(), app.name.clone(), folder.clone());
        pick.connect_clicked(move |_| choose_folder(&cc, &id, &name, &f));
        folder.add_suffix(&pick);
        row.add_row(&folder);
    }
    let tags = if app.tags.is_empty() { "none".to_string() } else { app.tags.join(", ") };
    row.add_row(&adw::ActionRow::builder().title("Good At").subtitle(glib::markup_escape_text(&format!("{tags} (edit in agents.toml)")).as_str()).build());
    row
}

fn choose_folder(ctx: &C, app_id: &str, name: &str, row: &adw::EntryRow) {
    let dialog = gtk::FileDialog::builder().title(format!("Working Folder for {name}").as_str()).build();
    let (cc, id, row) = (ctx.clone(), app_id.to_string(), row.clone());
    dialog.select_folder(Some(&ctx.window), None::<&gio::Cancellable>, move |result| {
        let Ok(folder) = result else { return }; // cancelled
        let Some(path) = folder.path() else { return };
        let mut path = path.to_string_lossy().into_owned();
        let home = config::home_dir().to_string_lossy().into_owned();
        if let Some(rest) = path.strip_prefix(&home) {
            path = format!("~{rest}");
        }
        row.set_text(&path);
        save_app(&cc, &id, "cwd", Scalar::Str(path));
    });
}

// --- Gateway ----------------------------------------------------------------------

fn gateway_page(ctx: &C) -> adw::PreferencesPage {
    let c = ctx.state.borrow().config.clone();
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("LiteLLM Gateway")
        .description("Optional. Connect a LiteLLM proxy to list API models (OpenAI, Anthropic API, LM Studio, Ollama…) as rows. Leave the URL empty to turn it off.")
        .build();
    let cc = ctx.clone();
    group.add(&entry_row("Gateway URL (e.g. http://127.0.0.1:4000)", &c.litellm_url, false, move |v| save(&cc, "gateway", "url", Scalar::Str(v))));
    let cc = ctx.clone();
    group.add(&entry_row("Chat URL ({model} and {prompt} are filled in)", &c.chat_url, false, move |v| save(&cc, "gateway", "chat_url", Scalar::Str(v))));
    let cc = ctx.clone();
    group.add(&entry_row("Auto Model", &c.auto_model, false, move |v| save(&cc, "gateway", "auto_model", Scalar::Str(v))));
    let cc = ctx.clone();
    let key_title = format!("Gateway Key (saved to secrets.env as {})", c.litellm_key_env);
    group.add(&entry_row(&key_title, c.secrets.get(&c.litellm_key_env).map(String::as_str).unwrap_or(""), true, move |v| save_key(&cc, &v)));
    let cc = ctx.clone();
    let status = button_row("Connection", "Not tested", "Test", None, move || test_gateway(&cc));
    *ctx.status_row.borrow_mut() = Some(status.clone());
    group.add(&status);
    page.add(&group);

    let help = adw::PreferencesGroup::new();
    help.add(&button_row("Setup Guide", "Running LiteLLM and a chat UI, and adding providers", "Open", Some("help-browser-symbolic"),
                         || {
                             let _ = platform::open_uri(&format!("{DOCS}PROVIDERS.md"));
                         }));
    page.add(&help);
    page
}

fn save_key(ctx: &C, value: &str) {
    let env = ctx.state.borrow().config.litellm_key_env.clone();
    match config::set_secret(&env, value, Some(&directory(ctx))) {
        Ok(()) => {
            ctx.state.borrow_mut().reload();
            toast(ctx, if value.is_empty() { "Key removed" } else { "Key saved" });
        }
        Err(e) => toast(ctx, &format!("Couldn't save the key: {e}")),
    }
}

fn test_gateway(ctx: &C) {
    let c = ctx.state.borrow().config.clone();
    let Some(row) = ctx.status_row.borrow().clone() else { return };
    if c.litellm_url.is_empty() {
        row.set_subtitle("No gateway URL set");
        return;
    }
    let models = platform::Gateway::default().models(&c);
    if models.is_empty() {
        row.set_subtitle(glib::markup_escape_text(&format!("Couldn't reach {}, or it lists no models", c.litellm_url)).as_str());
    } else {
        let names: Vec<&str> = models.iter().take(4).map(|m| m.name.as_str()).collect();
        let more = if models.len() > 4 { "…" } else { "" };
        row.set_subtitle(glib::markup_escape_text(&format!("Connected: {} models ({}{more})", models.len(), names.join(", "))).as_str());
    }
}
