"""The prompt box: a spotlight-style window for sending a prompt straight to an agent.

Opened from the app grid, a keyboard shortcut, or the search category's icon. It ranks the
same rows as the GNOME search provider, and stays running in the background after the first
use (for `[prompt] idle_minutes`), so it reopens instantly.
"""

import sys

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

from . import APP_ID, catalog, config as config_mod, launch, query as query_mod, ranker  # noqa: E402

CSS = """
.prompt-box { padding: 12px; }
.prompt-entry { font-size: 1.35em; min-height: 44px; }
.prompt-rows row { padding: 6px 4px; }
"""


class PromptWindow(Adw.ApplicationWindow):
    def __init__(self, app: "PromptApp"):
        super().__init__(application=app, title="Agent Search", resizable=False)
        self.app = app
        self.set_default_size(680, -1)
        self.set_hide_on_close(True)

        self.entry = Gtk.Entry(placeholder_text="Ask anything…  @model to choose · !high for effort",
                               hexpand=True)
        self.entry.add_css_class("prompt-entry")
        self.entry.set_icon_from_icon_name(Gtk.EntryIconPosition.PRIMARY, "system-search-symbolic")
        self.entry.connect("changed", lambda *_: self.refresh())
        self.entry.connect("activate", lambda *_: self.run_selected())

        self.rows = Gtk.ListBox(selection_mode=Gtk.SelectionMode.BROWSE)
        self.rows.add_css_class("boxed-list")
        self.rows.add_css_class("prompt-rows")
        self.rows.connect("row-activated", lambda _box, row: self.run(row))

        self.hint = Gtk.Label(label="Enter to send · ↑↓ to choose · Esc to close",
                              css_classes=["dim-label", "caption"], margin_top=4)

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10, css_classes=["prompt-box"])
        box.append(self.entry)
        box.append(self.rows)
        box.append(self.hint)
        self.set_content(box)

        keys = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        keys.connect("key-pressed", self.on_key)
        self.add_controller(keys)
        # Like a spotlight: clicking elsewhere closes it.
        self.connect("notify::is-active", lambda *_: self.is_active() or self.dismiss())
        self.refresh()

    # --- rows -----------------------------------------------------------------

    def refresh(self) -> None:
        self.rows.remove_all()
        q = query_mod.parse(self.entry.get_text().split())
        found = ranker.rank(q, catalog.candidates(self.app.config, self.app.gateway), self.app.config)
        for r in found:
            row = Adw.ActionRow(title=GLib.markup_escape_text(r.title),
                                subtitle=GLib.markup_escape_text(r.subtitle), activatable=True)
            row.add_prefix(Gtk.Image(icon_name=r.candidate.icon, pixel_size=32))
            row.result_id = r.result_id
            self.rows.append(row)
        self.rows.set_visible(bool(found))
        self.hint.set_visible(bool(found))
        if found:
            self.rows.select_row(self.rows.get_row_at_index(0))

    def run_selected(self) -> None:
        row = self.rows.get_selected_row()
        if row is not None:
            self.run(row)

    def run(self, row) -> None:
        prompt = query_mod.parse(self.entry.get_text().split()).prompt
        launch.activate(self.app.config, row.result_id, prompt)
        self.entry.set_text("")
        self.dismiss()

    def move(self, step: int) -> None:
        row = self.rows.get_selected_row()
        index = (row.get_index() if row else -1) + step
        target = self.rows.get_row_at_index(max(0, index))
        if target is not None:
            self.rows.select_row(target)

    # --- window -----------------------------------------------------------------

    def on_key(self, _ctl, keyval, _code, _state) -> bool:
        if keyval == Gdk.KEY_Escape:
            self.dismiss()
            return True
        if keyval in (Gdk.KEY_Down, Gdk.KEY_Tab):
            self.move(1)
            return True
        if keyval in (Gdk.KEY_Up, Gdk.KEY_ISO_Left_Tab):
            self.move(-1)
            return True
        return False

    def summon(self, text: str | None = None) -> None:
        if text is not None:
            self.entry.set_text(text)
        self.present()
        self.entry.grab_focus()
        self.entry.set_position(-1)

    def dismiss(self) -> None:
        self.set_visible(False)
        self.app.start_idle_timer()


class PromptApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id=APP_ID, flags=Gio.ApplicationFlags.DEFAULT_FLAGS)
        self.config = config_mod.load()
        self.gateway = catalog.Gateway(self.config)
        self.window: PromptWindow | None = None
        self._idle_source = 0

        search = Gio.SimpleAction.new("search", GLib.VariantType.new("s"))
        search.connect("activate", lambda _a, text: self.open(text.get_string()))
        self.add_action(search)

    def do_startup(self):
        Adw.Application.do_startup(self)
        css = Gtk.CssProvider()
        css.load_from_string(CSS)
        Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), css,
                                                  Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        self.hold()  # stay running while hidden, until the idle timer releases us

    def do_activate(self):
        self.open(None)

    def open(self, text: str | None) -> None:
        self.cancel_idle_timer()
        if self.window is None:
            self.window = PromptWindow(self)
        self.window.summon(text)

    def start_idle_timer(self) -> None:
        self.cancel_idle_timer()
        minutes = self.config.prompt_idle_minutes
        if minutes <= 0:
            self.quit()
        else:
            self._idle_source = GLib.timeout_add_seconds(int(minutes * 60), self._on_idle)

    def cancel_idle_timer(self) -> None:
        if self._idle_source:
            GLib.source_remove(self._idle_source)
            self._idle_source = 0

    def _on_idle(self) -> bool:
        self._idle_source = 0
        self.quit()
        return GLib.SOURCE_REMOVE


def main() -> int:
    return PromptApp().run(sys.argv)


if __name__ == "__main__":
    sys.exit(main())
