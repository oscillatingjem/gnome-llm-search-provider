"""The settings window, opened from the gear on the prompt box.

Every change is written straight into agents.toml, the single source of settings, keeping its
comments (see toml_edit.py). The prompt box and the search provider pick changes up without
restarting, and this window refreshes itself if agents.toml is edited while it's open.
"""

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GLib, Gtk  # noqa: E402

from . import catalog, config as config_mod, launch, shortcut, toml_edit  # noqa: E402

DOCS = "https://github.com/oscillatingjem/gnome-llm-search-provider/blob/main/docs/"


class SettingsWindow(Adw.Window):
    def __init__(self, app):
        super().__init__(application=app, title="Agent Search Settings")
        self.app = app
        self.set_default_size(600, 680)

        self.stack = Adw.ViewStack()
        self.build_pages()

        header = Adw.HeaderBar(title_widget=Adw.ViewSwitcher(stack=self.stack,
                                                             policy=Adw.ViewSwitcherPolicy.WIDE))
        self.toasts = Adw.ToastOverlay(child=self.stack)
        view = Adw.ToolbarView(content=self.toasts)
        view.add_top_bar(header)
        self.set_content(view)

        keys = Gtk.EventControllerKey()
        keys.connect("key-pressed", self.on_key)
        self.add_controller(keys)
        # Once closed (not while closing, when it still counts as visible), let the app idle out.
        self.connect("close-request", self.on_close)

        # Stay in sync with agents.toml when it's edited elsewhere (e.g. by hand) while open.
        self._refresh_source = 0
        self.monitor = Gio.File.new_for_path(str(self.directory / config_mod.AGENTS_FILE)).monitor_file(
            Gio.FileMonitorFlags.WATCH_MOVES, None)
        self.monitor.connect("changed", lambda *_: self.schedule_refresh())

    def build_pages(self) -> None:
        visible = self.stack.get_visible_child_name()
        pages = self.stack.get_pages()
        for child in [pages.get_item(i).get_child() for i in range(pages.get_n_items())]:
            self.stack.remove(child)
        self.stack.add_titled_with_icon(self.general_page(), "general", "General", "preferences-system-symbolic")
        self.stack.add_titled_with_icon(self.agents_page(), "agents", "Agents", "system-users-symbolic")
        self.stack.add_titled_with_icon(self.gateway_page(), "gateway", "Gateway", "network-server-symbolic")
        if visible:
            self.stack.set_visible_child_name(visible)

    def schedule_refresh(self) -> None:
        if not self._refresh_source:
            self._refresh_source = GLib.timeout_add(300, self.refresh_from_disk)

    def refresh_from_disk(self) -> bool:
        self._refresh_source = 0
        if config_mod.signature(self.directory) != self.app.signature:   # not our own save
            self.app.reload()
            self.build_pages()
            self.toast("Updated from agents.toml")
        return GLib.SOURCE_REMOVE

    def on_close(self, *_):
        self.monitor.cancel()
        GLib.idle_add(self.app.start_idle_timer)
        return False

    def on_key(self, _ctl, keyval, _code, _state) -> bool:
        if keyval == Gdk.KEY_Escape:
            self.close()
            return True
        return False

    @property
    def directory(self):
        return self.app.config.directory or config_mod.config_dir()

    # --- saving ---------------------------------------------------------------------

    def save(self, section: str, key: str, value) -> None:
        self._write([(section, key, None, value)])

    def save_app(self, app_id: str, key: str, value) -> None:
        self._write([("app", key, app_id, value)])

    def _write(self, edits) -> None:
        try:
            config_mod.update(edits, self.directory)
        except toml_edit.EditError as e:
            self.toast(f"Couldn't save: {e}")
            return
        except OSError as e:
            self.toast(f"Couldn't save: {e.strerror}")
            return
        self.app.reload()

    def toast(self, text: str) -> None:
        self.toasts.add_toast(Adw.Toast(title=GLib.markup_escape_text(text), timeout=3))

    # --- rows -------------------------------------------------------------------------

    def spin_row(self, title, subtitle, value, lower, upper, step, on_change, digits=0):
        row = Adw.SpinRow.new_with_range(lower, upper, step)
        row.set_title(title)
        row.set_subtitle(subtitle)
        row.set_digits(digits)
        row.set_value(value)
        row.connect("notify::value", lambda r, _p: on_change(r.get_value() if digits else int(r.get_value())))
        return row

    def entry_row(self, title, text, on_apply, password=False):
        row = (Adw.PasswordEntryRow if password else Adw.EntryRow)(title=title, show_apply_button=True)
        row.set_text(text)
        row.connect("apply", lambda r: on_apply(r.get_text().strip()))
        return row

    def button_row(self, title, subtitle, label, on_click, icon=None):
        row = Adw.ActionRow(title=title, subtitle=subtitle)
        button = Gtk.Button(valign=Gtk.Align.CENTER, tooltip_text=label)
        if icon:
            button.set_icon_name(icon)
            button.add_css_class("flat")
        else:
            button.set_label(label)
        button.connect("clicked", lambda *_: on_click())
        row.add_suffix(button)
        row.set_activatable_widget(button)
        return row

    # --- General ----------------------------------------------------------------------

    def general_page(self) -> Adw.PreferencesPage:
        c = self.app.config
        page = Adw.PreferencesPage()

        box = Adw.PreferencesGroup(title="Prompt Box")
        self.shortcut_row = Adw.ActionRow(title="Keyboard Shortcut",
                                          subtitle="Opens the prompt box from anywhere")
        self.shortcut_label = Gtk.ShortcutLabel(valign=Gtk.Align.CENTER, disabled_text="None")
        change = Gtk.Button(label="Change…", valign=Gtk.Align.CENTER)
        change.connect("clicked", lambda *_: ShortcutDialog(self).present())
        self.shortcut_row.add_suffix(self.shortcut_label)
        self.shortcut_row.add_suffix(change)
        self.refresh_shortcut()
        box.add(self.shortcut_row)
        box.add(self.spin_row("Keep Running After Closing", "Minutes; opens instantly within this time. 0 quits at once",
                              c.prompt_idle_minutes, 0, 480, 5,
                              lambda v: self.save("prompt", "idle_minutes", v)))
        page.add(box)

        results = Adw.PreferencesGroup(title="Results",
                                       description="Used by both the prompt box and GNOME search")
        results.add(self.spin_row("Minimum Characters", "Shorter prompts show no rows", c.min_chars, 1, 20, 1,
                                  lambda v: self.save("search", "min_chars", v)))
        results.add(self.spin_row("Rows Shown", "GNOME search shows about five", c.max_rows, 1, 10, 1,
                                  lambda v: self.save("search", "max_rows", v)))
        results.add(self.spin_row("Low Effort Up To", "Prompts with this many words or fewer get low effort",
                                  c.low_effort_max_words, 0, 50, 1,
                                  lambda v: self.save("routing", "low_effort_max_words", v)))
        page.add(results)

        files = Adw.PreferencesGroup(title="Configuration Files")
        files.add(self.button_row("agents.toml", "Add agents, aliases and ranking rules. Every setting is explained in the file",
                                  "Open", lambda: self.open_file(config_mod.AGENTS_FILE), "document-edit-symbolic"))
        files.add(self.button_row("Restore Defaults",
                                  "Puts the settings on this page back to factory values. "
                                  "Agents, rules, the gateway and the shortcut are kept",
                                  "Restore…", self.confirm_restore))
        files.add(self.button_row("Documentation", "Search syntax, providers and troubleshooting",
                                  "Open", lambda: launch.open_url(DOCS + "USAGE.md"), "help-browser-symbolic"))
        page.add(files)
        return page

    def refresh_shortcut(self) -> None:
        try:
            self.shortcut_label.set_accelerator(shortcut.get())
        except GLib.Error:
            self.shortcut_label.set_accelerator("")

    def set_shortcut(self, binding: str) -> None:
        try:
            shortcut.set(binding)
        except (RuntimeError, GLib.Error) as e:
            self.toast(f"Couldn't set the shortcut: {e}")
        self.refresh_shortcut()

    def open_file(self, name: str) -> None:
        launch.open_url(Gio.File.new_for_path(str(self.directory / name)).get_uri())

    def confirm_restore(self) -> None:
        dialog = Adw.AlertDialog(
            heading="Restore Defaults?",
            body="Minimum characters, rows shown, the low-effort limit and how long the box keeps "
                 "running go back to factory values in agents.toml. Your agents, aliases, ranking "
                 "rules, gateway and shortcut are kept.")
        dialog.add_response("cancel", "Cancel")
        dialog.add_response("restore", "Restore")
        dialog.set_response_appearance("restore", Adw.ResponseAppearance.DESTRUCTIVE)
        dialog.set_default_response("cancel")
        dialog.connect("response", lambda _d, r: r == "restore" and self.restore())
        dialog.present(self)

    def restore(self) -> None:
        try:
            config_mod.restore_defaults(self.directory)
        except (toml_edit.EditError, OSError) as e:
            self.toast(f"Couldn't restore defaults: {getattr(e, 'strerror', None) or e}")
            return
        self.app.reload()
        self.close()                       # rebuild so every row shows the restored values
        self.app.open_settings().toast("Defaults restored")

    # --- Agents -----------------------------------------------------------------------

    def agents_page(self) -> Adw.PreferencesPage:
        page = Adw.PreferencesPage()
        group = Adw.PreferencesGroup(title="Agents",
                                     description="Turn agents on or off, and change how they rank. "
                                                 "Higher priority ranks higher.")
        for app in self.app.config.apps:
            group.add(self.agent_row(app))
        page.add(group)

        more = Adw.PreferencesGroup()
        more.add(self.button_row("Add an Agent", "New agents are added in agents.toml, which has ready-made examples",
                                 "Open", lambda: self.open_file(config_mod.AGENTS_FILE), "list-add-symbolic"))
        page.add(more)
        return page

    def agent_row(self, app: config_mod.App) -> Adw.ExpanderRow:
        missing = [p for p in app.requires if not launch.which(p)]
        if missing:
            subtitle = "Not installed: " + ", ".join(missing)
        elif app.command:
            subtitle = "Runs " + " ".join(str(a) for a in app.command if isinstance(a, str) and "{" not in a)[:60]
        else:
            subtitle = "Opens " + app.url.split("?")[0]
        row = Adw.ExpanderRow(title=GLib.markup_escape_text(app.name), subtitle=GLib.markup_escape_text(subtitle),
                              show_enable_switch=True, enable_expansion=app.enabled)
        row.connect("notify::enable-expansion",
                    lambda r, _p: self.save_app(app.id, "enabled", r.get_enable_expansion()))
        row.add_row(self.spin_row("Priority", "Added to its ranking score", app.priority, -10, 10, 0.5,
                                  lambda v: self.save_app(app.id, "priority", v), digits=1))
        if app.command:
            folder = self.entry_row("Working Folder", app.cwd, lambda v: self.save_app(app.id, "cwd", v or "~"))
            pick = Gtk.Button(icon_name="folder-open-symbolic", valign=Gtk.Align.CENTER,
                              tooltip_text="Choose Folder", css_classes=["flat"])
            pick.connect("clicked", lambda *_: self.choose_folder(app, folder))
            folder.add_suffix(pick)
            row.add_row(folder)
        tags = ", ".join(app.tags) or "none"
        row.add_row(Adw.ActionRow(title="Good At", subtitle=GLib.markup_escape_text(f"{tags} (edit in agents.toml)")))
        return row

    def choose_folder(self, app: config_mod.App, row: Adw.EntryRow) -> None:
        dialog = Gtk.FileDialog(title=f"Working Folder for {app.name}")

        def done(d, result):
            try:
                folder = d.select_folder_finish(result)
            except GLib.Error:
                return  # cancelled
            path = folder.get_path()
            home = GLib.get_home_dir()
            if path.startswith(home):
                path = "~" + path[len(home):]
            row.set_text(path)
            self.save_app(app.id, "cwd", path)

        dialog.select_folder(self, None, done)

    # --- Gateway ----------------------------------------------------------------------

    def gateway_page(self) -> Adw.PreferencesPage:
        c = self.app.config
        page = Adw.PreferencesPage()
        group = Adw.PreferencesGroup(
            title="LiteLLM Gateway",
            description="Optional. Connect a LiteLLM proxy to list API models (OpenAI, Anthropic API, "
                        "LM Studio, Ollama…) as rows. Leave the URL empty to turn it off.")
        group.add(self.entry_row("Gateway URL (e.g. http://127.0.0.1:4000)", c.litellm_url,
                                 lambda v: self.save("gateway", "url", v)))
        group.add(self.entry_row("Chat URL ({model} and {prompt} are filled in)", c.chat_url,
                                 lambda v: self.save("gateway", "chat_url", v)))
        group.add(self.entry_row("Auto Model", c.auto_model, lambda v: self.save("gateway", "auto_model", v)))
        key_row = self.entry_row(f"Gateway Key (saved to secrets.env as {c.litellm_key_env})",
                                 c.secrets.get(c.litellm_key_env, ""), self.save_key, password=True)
        group.add(key_row)
        self.status_row = self.button_row("Connection", "Not tested", "Test", self.test_gateway)
        group.add(self.status_row)
        page.add(group)

        help_group = Adw.PreferencesGroup()
        help_group.add(self.button_row("Setup Guide", "Running LiteLLM and a chat UI, and adding providers",
                                       "Open", lambda: launch.open_url(DOCS + "PROVIDERS.md"), "help-browser-symbolic"))
        page.add(help_group)
        return page

    def save_key(self, value: str) -> None:
        try:
            config_mod.set_secret(self.app.config.litellm_key_env, value, self.directory)
        except OSError as e:
            self.toast(f"Couldn't save the key: {e.strerror}")
            return
        self.app.reload()
        self.toast("Key saved" if value else "Key removed")

    def test_gateway(self) -> None:
        c = self.app.config
        if not c.litellm_url:
            self.status_row.set_subtitle("No gateway URL set")
            return
        models = catalog.Gateway(c).models()
        if models:
            names = ", ".join(m.name for m in models[:4]) + ("…" if len(models) > 4 else "")
            self.status_row.set_subtitle(GLib.markup_escape_text(f"Connected: {len(models)} models ({names})"))
        else:
            self.status_row.set_subtitle(GLib.markup_escape_text(f"Couldn't reach {c.litellm_url}, or it lists no models"))


class ShortcutDialog(Adw.Window):
    """Press a key combination to set it, the way GNOME Settings does."""

    def __init__(self, parent: SettingsWindow):
        super().__init__(transient_for=parent, modal=True, title="Set Shortcut", resizable=False)
        self.parent_window = parent
        self.set_default_size(420, -1)
        status = Adw.StatusPage(icon_name="input-keyboard-symbolic", title="Press the New Shortcut",
                                description="Esc to cancel · Backspace to remove the shortcut")
        status.add_css_class("compact")
        view = Adw.ToolbarView(content=status)
        view.add_top_bar(Adw.HeaderBar())
        self.set_content(view)
        keys = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        keys.connect("key-pressed", self.on_key)
        self.add_controller(keys)

    def on_key(self, _ctl, keyval, keycode, state) -> bool:
        mods = state & Gtk.accelerator_get_default_mod_mask()
        if keyval == Gdk.KEY_Escape and not mods:
            self.close()
            return True
        if keyval == Gdk.KEY_BackSpace and not mods:
            self.parent_window.set_shortcut("")
            self.close()
            return True
        if keyval in MODIFIER_KEYS:
            return True  # wait for the full combination
        if not mods:
            return True  # a shortcut needs at least one modifier, or it would steal normal typing
        self.parent_window.set_shortcut(Gtk.accelerator_name(Gdk.keyval_to_lower(keyval), mods))
        self.close()
        return True


MODIFIER_KEYS = {getattr(Gdk, f"KEY_{k}") for k in (
    "Shift_L", "Shift_R", "Control_L", "Control_R", "Alt_L", "Alt_R", "Super_L", "Super_R",
    "Meta_L", "Meta_R", "Hyper_L", "Hyper_R", "ISO_Level3_Shift", "Caps_Lock")}
