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
use agent_search_core::structure::{ListItem, Op, Sel};
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
    ctx.stack.add_titled_with_icon(&routing_page(ctx), Some("routing"), "Routing", "view-sort-descending-symbolic");
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

// --- Structural edits ----------------------------------------------------------------

/// Apply structural edits; `rebuild` redraws the pages (for adds, deletes, renames and lists).
fn write_ops(ctx: &C, ops: Vec<Op>, rebuild: bool) -> bool {
    match config::apply_ops(&ops, Some(&directory(ctx))) {
        Ok(()) => {
            ctx.state.borrow_mut().reload();
            if rebuild {
                // Rebuild after this handler returns: the row that fired it is about to be removed.
                let weak = Rc::downgrade(ctx);
                glib::idle_add_local_once(move || {
                    if let Some(ctx) = weak.upgrade() {
                        build_pages(&ctx);
                    }
                });
            }
            true
        }
        Err(e) => {
            toast(ctx, &format!("Couldn't save: {e}"));
            false
        }
    }
}

fn app_sel(id: &str) -> Sel {
    Sel::Id(id.to_string())
}

/// "a, b, c" → list items (empty entries dropped).
fn comma_items(text: &str) -> Vec<ListItem> {
    text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(|s| ListItem::Str(s.to_string())).collect()
}

/// Save a comma-separated list field of an agent; an empty list removes the key.
fn save_app_list(ctx: &C, id: &str, key: &str, text: &str) {
    let items = comma_items(text);
    let op = if items.is_empty() {
        Op::Remove { table: "app".into(), sel: app_sel(id), key: key.into() }
    } else {
        Op::SetList { table: "app".into(), sel: app_sel(id), key: key.into(), items }
    };
    write_ops(ctx, vec![op], false);
}

/// Command text as TOML: ["prog", "--", ["--model", "{model}"], "{prompt}"].
fn command_to_text(cmd: &[config::CommandItem]) -> String {
    let items: Vec<ListItem> = cmd
        .iter()
        .map(|c| match c {
            config::CommandItem::Arg(a) => ListItem::Str(a.clone()),
            config::CommandItem::Group(g) => ListItem::Group(g.clone()),
        })
        .collect();
    if items.is_empty() { String::new() } else { agent_search_core::structure::format_list(&items).replace("\n  ", " ").replace(",\n]", "]").replace("[\n", "[") }
}

fn parse_command(text: &str) -> Result<Vec<ListItem>, String> {
    let table: toml::Table = format!("x = {text}").parse().map_err(|_| "Write the command as a list, e.g. [\"kgx\", \"--\", \"claude\", \"{prompt}\"]".to_string())?;
    let Some(toml::Value::Array(items)) = table.get("x") else { return Err("The command must be a list in [ ]".into()) };
    items
        .iter()
        .map(|v| match v {
            toml::Value::String(s) => Ok(ListItem::Str(s.clone())),
            toml::Value::Array(g) => g
                .iter()
                .map(|x| x.as_str().map(str::to_string).ok_or_else(|| "Optional groups may only hold text".to_string()))
                .collect::<Result<Vec<_>, _>>()
                .map(ListItem::Group),
            _ => Err("Command items must be text, or [ ] groups of text".into()),
        })
        .collect()
}

/// Mark an entry red while its text isn't a valid routing pattern.
fn regex_feedback(row: &adw::EntryRow) {
    row.connect_changed(|r| {
        if agent_search_core::pyre::compile(&r.text()).is_ok() || r.text().is_empty() {
            r.remove_css_class("error");
        } else {
            r.add_css_class("error");
        }
    });
}

fn delete_button(tooltip: &str, on_click: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::builder().icon_name("user-trash-symbolic").valign(gtk::Align::Center).tooltip_text(tooltip).build();
    b.add_css_class("flat");
    b.connect_clicked(move |_| on_click());
    b
}

fn confirm(ctx: &C, heading: &str, body: &str, action: &str, on_yes: impl Fn() + 'static) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("yes", action);
    dialog.set_response_appearance("yes", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.connect_response(None, move |_, r| {
        if r == "yes" {
            on_yes();
        }
    });
    dialog.present(Some(&ctx.window));
}

/// Rows editing a list of regex patterns: one row per pattern, plus an "Add pattern" row.
fn pattern_rows(ctx: &C, patterns: &[String], save: Rc<dyn Fn(&C, Vec<String>)>) -> Vec<gtk::Widget> {
    let mut rows: Vec<gtk::Widget> = Vec::new();
    for (i, p) in patterns.iter().enumerate() {
        let (cc, all, s) = (ctx.clone(), patterns.to_vec(), save.clone());
        let row = entry_row(&format!("Pattern {}", i + 1), p, false, move |v| {
            if let Err(e) = agent_search_core::pyre::compile(&v) {
                toast(&cc, &format!("Not a valid pattern: {e}"));
                return;
            }
            let mut next = all.clone();
            next[i] = v;
            s(&cc, next);
        });
        regex_feedback(&row);
        let (cc, all, s) = (ctx.clone(), patterns.to_vec(), save.clone());
        row.add_suffix(&delete_button("Remove Pattern", move || {
            let mut next = all.clone();
            next.remove(i);
            s(&cc, next);
        }));
        rows.push(row.upcast());
    }
    let (cc, all, s) = (ctx.clone(), patterns.to_vec(), save.clone());
    let add = entry_row("Add a pattern (regular expression)", "", false, move |v| {
        if v.is_empty() {
            return;
        }
        if let Err(e) = agent_search_core::pyre::compile(&v) {
            toast(&cc, &format!("Not a valid pattern: {e}"));
            return;
        }
        let mut next = all.clone();
        next.push(v);
        s(&cc, next);
    });
    regex_feedback(&add);
    rows.push(add.upcast());
    rows
}

// --- Agents -----------------------------------------------------------------------

fn agents_page(ctx: &C) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title("Agents")
        .description("Turn agents on or off and edit everything about them. Changes are saved to agents.toml, keeping its comments.")
        .build();
    let apps = ctx.state.borrow().config.apps.clone();
    for app in &apps {
        group.add(&agent_row(ctx, app));
    }
    page.add(&group);
    page.add(&add_agent_group(ctx));
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
        format!("Runs {}", words.join(" ")).chars().take(65).collect()
    } else if app.url.is_empty() {
        "Not set up: add a link or a command".to_string()
    } else {
        format!("Opens {}", app.url.split('?').next().unwrap_or(""))
    };
    let row = adw::ExpanderRow::builder()
        .title(glib::markup_escape_text(&app.name).as_str())
        .subtitle(glib::markup_escape_text(&subtitle).as_str())
        .show_enable_switch(true)
        .enable_expansion(app.enabled)
        .build();
    row.add_prefix(&gtk::Image::builder().icon_name(app.icon.as_str()).pixel_size(24).build());
    let id = app.id.clone();

    let (cc, i) = (ctx.clone(), id.clone());
    row.connect_enable_expansion_notify(move |r| save_app(&cc, &i, "enabled", Scalar::Bool(r.enables_expansion())));

    let (cc, i) = (ctx.clone(), id.clone());
    row.add_row(&entry_row("Name", &app.name, false, move |v| {
        if v.is_empty() {
            toast(&cc, "The name can't be empty");
        } else {
            save_app(&cc, &i, "name", Scalar::Str(v));
            build_pages_later(&cc);
        }
    }));
    let (cc, i) = (ctx.clone(), id.clone());
    row.add_row(&entry_row("ID (also matches @id)", &app.id, false, move |v| {
        if v != i {
            write_ops(&cc, vec![Op::RenameApp { from: i.clone(), to: v }], true);
        }
    }));
    let (cc, i) = (ctx.clone(), id.clone());
    row.add_row(&entry_row("Link ({prompt}, {model}, {effort})", &app.url, false, move |v| {
        let op = if v.is_empty() {
            Op::Remove { table: "app".into(), sel: app_sel(&i), key: "url".into() }
        } else {
            Op::Set { table: "app".into(), sel: app_sel(&i), key: "url".into(), value: Scalar::Str(v) }
        };
        write_ops(&cc, vec![op], true);
    }));
    let (cc, i) = (ctx.clone(), id.clone());
    row.add_row(&entry_row("Command (a list; [ ] groups are optional)", &command_to_text(&app.command), false, move |v| {
        let op = if v.is_empty() {
            Op::Remove { table: "app".into(), sel: app_sel(&i), key: "command".into() }
        } else {
            match parse_command(&v) {
                Ok(items) => Op::SetList { table: "app".into(), sel: app_sel(&i), key: "command".into(), items },
                Err(e) => return toast(&cc, &e),
            }
        };
        write_ops(&cc, vec![op], true);
    }));

    let (cc, i) = (ctx.clone(), id.clone());
    let folder = entry_row("Working Folder (for commands)", &app.cwd, false, move |v| {
        save_app(&cc, &i, "cwd", Scalar::Str(if v.is_empty() { "~".into() } else { v }))
    });
    let pick = gtk::Button::builder().icon_name("folder-open-symbolic").valign(gtk::Align::Center).tooltip_text("Choose Folder").build();
    pick.add_css_class("flat");
    let (cc, i, name, f) = (ctx.clone(), id.clone(), app.name.clone(), folder.clone());
    pick.connect_clicked(move |_| choose_folder(&cc, &i, &name, &f));
    folder.add_suffix(&pick);
    row.add_row(&folder);

    for (title, key, value) in [
        ("Requires (programs, comma-separated)", "requires", app.requires.join(", ")),
        ("Good At (tags, comma-separated)", "tags", app.tags.join(", ")),
        ("Models (for @model, comma-separated)", "models", app.models.join(", ")),
    ] {
        let (cc, i) = (ctx.clone(), id.clone());
        row.add_row(&entry_row(title, &value, false, move |v| save_app_list(&cc, &i, key, &v)));
    }
    let (cc, i) = (ctx.clone(), id.clone());
    row.add_row(&entry_row("Effort Levels (low, medium, high, xhigh, max)", &app.efforts.join(", "), false, move |v| {
        let bad: Vec<String> = comma_items(&v)
            .into_iter()
            .filter_map(|x| match x {
                ListItem::Str(s) if !agent_search_core::query::EFFORTS.contains(&s.as_str()) => Some(s),
                _ => None,
            })
            .collect();
        if bad.is_empty() {
            save_app_list(&cc, &i, "efforts", &v);
        } else {
            toast(&cc, &format!("Unknown effort level: {}", bad.join(", ")));
        }
    }));
    let (cc, i) = (ctx.clone(), id.clone());
    let icon = entry_row("Icon (name from your icon theme)", &app.icon, false, move |v| {
        save_app(&cc, &i, "icon", Scalar::Str(if v.is_empty() { "system-search-symbolic".into() } else { v }));
        build_pages_later(&cc);
    });
    row.add_row(&icon);
    let (cc, i) = (ctx.clone(), id.clone());
    row.add_row(&spin_row("Priority", "Added to its ranking score; higher ranks higher", app.priority, -10.0, 10.0, 0.5, 1,
                          move |v| save_app(&cc, &i, "priority", Scalar::Float(v))));
    let copy = adw::SwitchRow::builder()
        .title("Copy Prompt")
        .subtitle("Also copy the prompt to the clipboard, for apps that can't receive it")
        .active(app.copy_prompt)
        .build();
    let (cc, i) = (ctx.clone(), id.clone());
    copy.connect_active_notify(move |s| save_app(&cc, &i, "copy_prompt", Scalar::Bool(s.is_active())));
    row.add_row(&copy);

    let (cc, i, name) = (ctx.clone(), id.clone(), app.name.clone());
    row.add_row(&button_row("Delete Agent", "Removes it, and its comment block, from agents.toml", "Delete…", Some("user-trash-symbolic"), move || {
        let (c2, i2) = (cc.clone(), i.clone());
        confirm(&cc, &format!("Delete {name}?"), "The agent and the comments directly above it are removed from agents.toml.", "Delete", move || {
            write_ops(&c2, vec![Op::DeleteApp { id: i2.clone() }], true);
        });
    }));
    row
}

fn build_pages_later(ctx: &C) {
    let weak = Rc::downgrade(ctx);
    glib::idle_add_local_once(move || {
        if let Some(ctx) = weak.upgrade() {
            build_pages(&ctx);
        }
    });
}

fn add_agent_group(ctx: &C) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title("Add an Agent")
        .description("Give it an id and a name, plus a link (e.g. https://site/?q={prompt}) or a command list. Everything else can be set afterwards.")
        .build();
    let id = adw::EntryRow::builder().title("ID (letters, digits, - and _)").build();
    let name = adw::EntryRow::builder().title("Name").build();
    let target = adw::EntryRow::builder().title("Link, or a command list in [ ]").build();
    group.add(&id);
    group.add(&name);
    group.add(&target);
    let (cc, id_r, name_r, target_r) = (ctx.clone(), id.clone(), name.clone(), target.clone());
    group.add(&button_row("Add Agent", "Added after the last agent in agents.toml", "Add", Some("list-add-symbolic"), move || {
        let (id, name, target) = (id_r.text().trim().to_string(), name_r.text().trim().to_string(), target_r.text().trim().to_string());
        if !agent_search_core::structure::valid_id(&id) {
            return toast(&cc, "The ID may only use letters, digits, - and _");
        }
        if name.is_empty() || target.is_empty() {
            return toast(&cc, "Fill in the name and a link or command");
        }
        let (url, command) = if target.starts_with('[') {
            match parse_command(&target) {
                Ok(items) => (None, Some(items)),
                Err(e) => return toast(&cc, &e),
            }
        } else {
            (Some(target), None)
        };
        if write_ops(&cc, vec![Op::AddApp { id, name, url, command }], true) {
            toast(&cc, "Agent added");
        }
    }));
    let cc = ctx.clone();
    group.add(&button_row("Edit agents.toml", "For anything else: every setting is explained in the file, with examples",
                          "Open", Some("document-edit-symbolic"), move || open_file(&cc, config::AGENTS_FILE)));
    group
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

// --- Routing ----------------------------------------------------------------------

fn routing_page(ctx: &C) -> adw::PreferencesPage {
    let c = ctx.state.borrow().config.clone();
    let page = adw::PreferencesPage::new();

    // Aliases
    let aliases = adw::PreferencesGroup::builder()
        .title("Aliases")
        .description("Your own @names. A target is an agent id, \"agent-id:model\", or a gateway model.")
        .build();
    let mut seen = std::collections::HashSet::new();
    for (name, target) in c.aliases.iter().rev() {
        if !seen.insert(name.clone()) {
            continue; // a later duplicate wins, as when loading
        }
        let (cc, n) = (ctx.clone(), name.clone());
        let row = entry_row(&format!("@{name}"), target, false, move |v| {
            if v.is_empty() {
                return toast(&cc, "Use the delete button to remove an alias");
            }
            write_ops(&cc, vec![Op::Set { table: "aliases".into(), sel: Sel::Table, key: n.clone(), value: Scalar::Str(v) }], false);
        });
        let (cc, n) = (ctx.clone(), name.clone());
        row.add_suffix(&delete_button("Remove Alias", move || {
            write_ops(&cc, vec![Op::Remove { table: "aliases".into(), sel: Sel::Table, key: n.clone() }], true);
        }));
        aliases.add(&row);
    }
    let new_name = adw::EntryRow::builder().title("New alias name (without @)").build();
    let new_target = adw::EntryRow::builder().title("Target").build();
    aliases.add(&new_name);
    aliases.add(&new_target);
    let (cc, n, t) = (ctx.clone(), new_name.clone(), new_target.clone());
    aliases.add(&button_row("Add Alias", "", "Add", Some("list-add-symbolic"), move || {
        let (name, target) = (n.text().trim().to_lowercase(), t.text().trim().to_string());
        if !agent_search_core::structure::valid_id(&name) || target.is_empty() {
            return toast(&cc, "Give the alias a name (letters, digits, - and _) and a target");
        }
        write_ops(&cc, vec![Op::Set { table: "aliases".into(), sel: Sel::Table, key: name, value: Scalar::Str(target) }], true);
    }));
    page.add(&aliases);

    // Effort
    let effort = adw::PreferencesGroup::builder()
        .title("Suggested Effort")
        .description("Used when you don't type !level. High if any pattern below matches, low for short prompts, otherwise medium.")
        .build();
    let cc = ctx.clone();
    effort.add(&spin_row("Low Effort Up To", "Prompts with this many words or fewer get low effort", c.low_effort_max_words as f64, 0.0, 50.0, 1.0, 0,
                         move |v| save(&cc, "routing", "low_effort_max_words", Scalar::Int(v as i64))));
    let save_high: Rc<dyn Fn(&C, Vec<String>)> = Rc::new(|ctx: &C, list: Vec<String>| {
        let items: Vec<ListItem> = list.into_iter().map(ListItem::Str).collect();
        write_ops(ctx, vec![Op::SetList { table: "routing".into(), sel: Sel::Table, key: "high_effort_patterns".into(), items }], true);
    });
    for row in pattern_rows(ctx, &c.high_effort_patterns, save_high) {
        effort.add(&row);
    }
    page.add(&effort);

    // Rules
    let rules = adw::PreferencesGroup::builder()
        .title("Ranking Rules")
        .description("When any pattern matches the prompt, agents tagged with the rule's tag get its weight added to their score.")
        .build();
    for (index, rule) in c.rules.iter().enumerate() {
        let row = adw::ExpanderRow::builder()
            .title(glib::markup_escape_text(&rule.tag).as_str())
            .subtitle(format!("weight {} · {} pattern{}", agent_search_core::pystr::repr_float(rule.weight), rule.patterns.len(), if rule.patterns.len() == 1 { "" } else { "s" }).as_str())
            .build();
        let sel = Sel::Index(index);
        let (cc, s) = (ctx.clone(), sel.clone());
        row.add_row(&entry_row("Tag", &rule.tag, false, move |v| {
            if v.is_empty() {
                return toast(&cc, "The tag can't be empty");
            }
            write_ops(&cc, vec![Op::Set { table: "routing.rule".into(), sel: s.clone(), key: "tag".into(), value: Scalar::Str(v) }], true);
        }));
        let (cc, s) = (ctx.clone(), sel.clone());
        row.add_row(&spin_row("Weight", "Added to matching agents' scores", rule.weight, -10.0, 20.0, 0.5, 1, move |v| {
            write_ops(&cc, vec![Op::Set { table: "routing.rule".into(), sel: s.clone(), key: "weight".into(), value: Scalar::Float(v) }], false);
        }));
        let s = sel.clone();
        let save_rule: Rc<dyn Fn(&C, Vec<String>)> = Rc::new(move |ctx: &C, list: Vec<String>| {
            if list.is_empty() {
                return toast(ctx, "A rule needs at least one pattern; delete the rule instead");
            }
            let items: Vec<ListItem> = list.into_iter().map(ListItem::Str).collect();
            write_ops(ctx, vec![Op::SetList { table: "routing.rule".into(), sel: s.clone(), key: "patterns".into(), items }], true);
        });
        for r in pattern_rows(ctx, &rule.patterns, save_rule) {
            row.add_row(&r);
        }
        let (cc, tag) = (ctx.clone(), rule.tag.clone());
        row.add_row(&button_row("Delete Rule", "", "Delete…", Some("user-trash-symbolic"), move || {
            let c2 = cc.clone();
            confirm(&cc, &format!("Delete the \"{tag}\" rule?"), "The rule and the comments directly above it are removed from agents.toml.", "Delete", move || {
                write_ops(&c2, vec![Op::DeleteRule { index }], true);
            });
        }));
        rules.add(&row);
    }
    let tag = adw::EntryRow::builder().title("New rule: tag").build();
    let pattern = adw::EntryRow::builder().title("New rule: first pattern").build();
    regex_feedback(&pattern);
    rules.add(&tag);
    rules.add(&pattern);
    let (cc, t, p) = (ctx.clone(), tag.clone(), pattern.clone());
    rules.add(&button_row("Add Rule", "Weight starts at 1; change it after adding", "Add", Some("list-add-symbolic"), move || {
        let (tag, pattern) = (t.text().trim().to_string(), p.text().to_string());
        if tag.is_empty() || pattern.is_empty() {
            return toast(&cc, "Fill in a tag and a pattern");
        }
        if let Err(e) = agent_search_core::pyre::compile(&pattern) {
            return toast(&cc, &format!("Not a valid pattern: {e}"));
        }
        write_ops(&cc, vec![Op::AddRule { tag, weight: 1.0, patterns: vec![pattern] }], true);
    }));
    page.add(&rules);
    page
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
    group.add(&entry_row("Key Name in secrets.env", &c.litellm_key_env, false, move |v| {
        if v.is_empty() || !v.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return toast(&cc, "Use letters, digits and _ only, e.g. LITELLM_MASTER_KEY");
        }
        save(&cc, "gateway", "key_env", Scalar::Str(v));
        build_pages_later(&cc);
    }));
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
