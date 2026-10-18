//! The prompt box's keyboard shortcut, stored as a GNOME custom shortcut (also editable in
//! Settings → Keyboard → Custom Shortcuts).

use agent_search_core::Identity;
use agent_search_core::config::home_dir;
use gio::prelude::*;

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";

fn path() -> String {
    format!("/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/{}/", Identity::current().shortcut_slug)
}

fn available() -> bool {
    gio::SettingsSchemaSource::default()
        .map(|s| s.lookup(MEDIA_KEYS, true).is_some() && s.lookup(CUSTOM, true).is_some())
        .unwrap_or(false)
}

/// The current binding, e.g. "<Primary><Super>space", or "" if there is none.
pub fn get() -> String {
    if !available() {
        return String::new();
    }
    let path = path();
    let listed = gio::Settings::new(MEDIA_KEYS).strv("custom-keybindings").iter().any(|p| p.as_str() == path);
    if !listed {
        return String::new();
    }
    gio::Settings::with_path(CUSTOM, &path).string("binding").to_string()
}

/// Set the binding; an empty string removes the shortcut.
pub fn set(binding: &str) -> Result<(), String> {
    if !available() {
        return Err("GNOME's keyboard settings schemas aren't installed".into());
    }
    let id = Identity::current();
    let path = path();
    let media = gio::Settings::new(MEDIA_KEYS);
    let paths: Vec<String> = media.strv("custom-keybindings").iter().map(|p| p.to_string()).collect();
    let custom = gio::Settings::with_path(CUSTOM, &path);
    let err = |e: glib::BoolError| e.to_string();
    if binding.is_empty() {
        let kept: Vec<&str> = paths.iter().map(String::as_str).filter(|p| *p != path).collect();
        media.set_strv("custom-keybindings", kept.as_slice()).map_err(err)?;
        for key in ["name", "command", "binding"] {
            custom.reset(key);
        }
    } else {
        custom.set_string("name", &id.name).map_err(err)?;
        let command = format!("{}/.local/bin/{} prompt", home_dir().display(), id.cli_name);
        custom.set_string("command", &command).map_err(err)?;
        custom.set_string("binding", binding).map_err(err)?;
        if !paths.contains(&path) {
            let mut all: Vec<&str> = paths.iter().map(String::as_str).collect();
            all.push(&path);
            media.set_strv("custom-keybindings", all.as_slice()).map_err(err)?;
        }
    }
    gio::Settings::sync();
    Ok(())
}
