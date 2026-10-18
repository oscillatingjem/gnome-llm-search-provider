"""GNOME Shell search provider (org.gnome.Shell.SearchProvider2) for LLMs and agents."""

import logging
import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

from . import APP_ID, PROVIDER_ID, catalog, config as config_mod, launch, query as query_mod, ranker  # noqa: E402
IDLE_TIMEOUT_MS = 60_000

INTERFACE_XML = """
<node>
  <interface name="org.gnome.Shell.SearchProvider2">
    <method name="GetInitialResultSet">
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetSubsearchResultSet">
      <arg type="as" name="previous_results" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="as" name="results" direction="out"/>
    </method>
    <method name="GetResultMetas">
      <arg type="as" name="identifiers" direction="in"/>
      <arg type="aa{sv}" name="metas" direction="out"/>
    </method>
    <method name="ActivateResult">
      <arg type="s" name="identifier" direction="in"/>
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
    <method name="LaunchSearch">
      <arg type="as" name="terms" direction="in"/>
      <arg type="u" name="timestamp" direction="in"/>
    </method>
  </interface>
</node>
"""

log = logging.getLogger("llm-search")


class SearchProvider:
    def __init__(self):
        self.config = config_mod.load()
        self.gateway = catalog.Gateway(self.config)
        self._signature = config_mod.signature()
        self.rows: dict[str, ranker.Row] = {}
        self.prompt = ""

    def reload_if_changed(self) -> None:
        sig = config_mod.signature()
        if sig != self._signature:
            self.config = config_mod.load()
            self.gateway = catalog.Gateway(self.config)
            self._signature = sig

    def search(self, terms: list[str]) -> list[str]:
        self.reload_if_changed()
        q = query_mod.parse(terms)
        rows = ranker.rank(q, catalog.candidates(self.config, self.gateway), self.config)
        self.prompt = q.prompt
        self.rows = {r.result_id: r for r in rows}
        return list(self.rows)

    def metas(self, ids: list[str]) -> list[dict[str, GLib.Variant]]:
        out = []
        for rid in ids:
            row = self.rows.get(rid)
            if row is None:
                continue
            description = f"“{self.prompt}”"
            if row.subtitle:
                description += f" · {row.subtitle}"
            out.append({
                "id": GLib.Variant("s", rid),
                "name": GLib.Variant("s", row.title),
                "description": GLib.Variant("s", description),
                "gicon": GLib.Variant("s", Gio.ThemedIcon.new(row.candidate.icon).to_string()),
            })
        return out

    def activate(self, rid: str, terms: list[str]) -> None:
        launch.activate(self.config, rid, query_mod.parse(terms).prompt)

    def launch_search(self, terms: list[str]) -> None:
        """Clicking the category icon: open the prompt box with the query, to refine it there."""
        Gio.bus_get_sync(Gio.BusType.SESSION).call(
            APP_ID, "/" + APP_ID.replace(".", "/"), "org.freedesktop.Application", "ActivateAction",
            GLib.Variant("(sava{sv})", ("search", [GLib.Variant("s", " ".join(terms))], {})),
            None, Gio.DBusCallFlags.NONE, -1, None, None, None)


class Application(Gio.Application):
    def __init__(self):
        super().__init__(application_id=PROVIDER_ID, flags=Gio.ApplicationFlags.IS_SERVICE)
        self.set_inactivity_timeout(IDLE_TIMEOUT_MS)
        self.provider = SearchProvider()
        self._registration = 0

    def do_dbus_register(self, connection, object_path):
        info = Gio.DBusNodeInfo.new_for_xml(INTERFACE_XML).interfaces[0]
        self._registration = connection.register_object(object_path, info, self._on_call)
        return Gio.Application.do_dbus_register(self, connection, object_path)

    def do_dbus_unregister(self, connection, object_path):
        if self._registration:
            connection.unregister_object(self._registration)
            self._registration = 0
        Gio.Application.do_dbus_unregister(self, connection, object_path)

    def _on_call(self, _conn, _sender, _path, _iface, method, params, invocation):
        self.hold()
        try:
            args = params.unpack()
            p = self.provider
            if method == "GetInitialResultSet":
                invocation.return_value(GLib.Variant("(as)", (p.search(args[0]),)))
            elif method == "GetSubsearchResultSet":
                invocation.return_value(GLib.Variant("(as)", (p.search(args[1]),)))
            elif method == "GetResultMetas":
                invocation.return_value(GLib.Variant("(aa{sv})", (p.metas(args[0]),)))
            elif method == "ActivateResult":
                p.activate(args[0], args[1])
                invocation.return_value(None)
            elif method == "LaunchSearch":
                p.launch_search(args[0])
                invocation.return_value(None)
            else:
                invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)
        except Exception as e:  # never let one bad query kill the service
            log.exception("%s failed", method)
            invocation.return_dbus_error("org.freedesktop.DBus.Error.Failed", str(e))
        finally:
            self.release()


def main() -> int:
    logging.basicConfig(level=logging.INFO, format="%(name)s: %(message)s")
    return Application().run(sys.argv)


if __name__ == "__main__":
    sys.exit(main())
