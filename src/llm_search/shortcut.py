"""The prompt box's keyboard shortcut, stored as a GNOME custom shortcut.

The installer (`install --shortcut`) and the settings window both manage the same entry, which
also shows up, and can be edited, in Settings → Keyboard → Custom Shortcuts.
"""

from pathlib import Path

from gi.repository import Gio

MEDIA_KEYS = "org.gnome.settings-daemon.plugins.media-keys"
CUSTOM = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding"
PATH = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/llm-search/"


def _available() -> bool:
    source = Gio.SettingsSchemaSource.get_default()
    return bool(source and source.lookup(MEDIA_KEYS, True) and source.lookup(CUSTOM, True))


def get() -> str:
    """The current binding, e.g. '<Primary><Super>space', or '' if there is none."""
    if not _available() or PATH not in Gio.Settings.new(MEDIA_KEYS).get_strv("custom-keybindings"):
        return ""
    return Gio.Settings.new_with_path(CUSTOM, PATH).get_string("binding")


def set(binding: str, name: str = "Agent Search") -> None:  # noqa: A001 (module-level API)
    """Set the binding; an empty string removes the shortcut."""
    if not _available():
        raise RuntimeError("GNOME's keyboard settings schemas aren't installed")
    media = Gio.Settings.new(MEDIA_KEYS)
    paths = media.get_strv("custom-keybindings")
    custom = Gio.Settings.new_with_path(CUSTOM, PATH)
    if not binding:
        media.set_strv("custom-keybindings", [p for p in paths if p != PATH])
        for key in ("name", "command", "binding"):
            custom.reset(key)
        return
    custom.set_string("name", name)
    custom.set_string("command", f"{Path.home()}/.local/bin/llm-search prompt")
    custom.set_string("binding", binding)
    if PATH not in paths:
        media.set_strv("custom-keybindings", [*paths, PATH])
