"""Build the URL or command for a chosen row, and open or run it."""

import os
import re
import shutil
import subprocess
from pathlib import Path
from urllib.parse import parse_qsl, quote, urlencode, urlsplit, urlunsplit

from .config import App, Config
from .ranker import APP, AUTO, MODEL

PLACEHOLDER = re.compile(r"\{(\w+)\}")
# D-Bus starts the provider with the session's PATH, which often lacks per-user bin folders.
EXTRA_PATH = ("~/.local/bin", "~/bin")


def fill(template: str, **values: str | None) -> str:
    """Fill {placeholders} (URL-encoded), then drop query parameters that ended up empty."""
    url = template.format(**{k: quote(v or "", safe="") for k, v in values.items()})
    parts = urlsplit(url)
    if not parts.query:
        return url
    query = [(k, v) for k, v in parse_qsl(parts.query, keep_blank_values=True) if v]
    return urlunsplit(parts._replace(query=urlencode(query, quote_via=quote)))


def which(program: str) -> str | None:
    """Find a program on PATH or in the per-user bin folders."""
    path = os.pathsep.join([os.environ.get("PATH", ""), *(os.path.expanduser(p) for p in EXTRA_PATH)])
    return shutil.which(program, path=path)


def app_available(app: App) -> bool:
    return all(which(p) for p in app.requires)


def fill_command(template: list, **values: str | None) -> list[str]:
    """Fill an argv template. Items are never passed through a shell.

    An item whose placeholder is empty is dropped; a nested list is an optional group,
    dropped as a whole if any placeholder in it is empty (e.g. ["--model", "{model}"]).
    """
    def render(item: str) -> str | None:
        names = PLACEHOLDER.findall(item)
        if any(not values.get(n) for n in names):
            return None
        return PLACEHOLDER.sub(lambda m: values[m.group(1)] or "", item)

    argv: list[str] = []
    for item in template:
        group = item if isinstance(item, list) else [item]
        rendered = [render(str(part)) for part in group]
        if all(r is not None for r in rendered):
            argv.extend(rendered)
    return argv


def command_for(app: App, model: str | None, effort: str | None, prompt: str) -> tuple[list[str], str]:
    """Build (argv, cwd) for a command app, resolving `requires` programs to absolute paths."""
    argv = fill_command(app.command, prompt=prompt, model=model, effort=effort)
    resolved = {p: which(p) for p in app.requires}
    argv = [resolved.get(a) or a for a in argv]
    return argv, str(Path(app.cwd).expanduser())


def run_command(argv: list[str], cwd: str) -> None:
    subprocess.Popen(argv, cwd=cwd, start_new_session=True, stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def gateway_model_id(base: str, effort: str | None) -> str:
    return f"{base}@{effort}" if effort else base


def url_for(config: Config, cid: str, model: str | None, effort: str | None, prompt: str) -> str | None:
    kind, _, name = cid.partition(":")
    if kind == APP:
        app = next((a for a in config.apps if a.id == name), None)
        return fill(app.url, prompt=prompt, model=model, effort=effort) if app and app.url else None
    if not config.chat_url:
        return None
    if kind == AUTO:
        return fill(config.chat_url, model=config.auto_model, prompt=prompt)
    if kind == MODEL:
        return fill(config.chat_url, model=gateway_model_id(name, effort), prompt=prompt)
    return None


def copy_to_clipboard(text: str) -> None:
    if shutil.which("wl-copy"):
        subprocess.run(["wl-copy", "--", text], check=False)
    elif shutil.which("xclip"):
        subprocess.run(["xclip", "-selection", "clipboard"], input=text.encode(), check=False)


def open_url(url: str) -> None:
    from gi.repository import Gio  # imported here so the rest of the module is testable without GI
    Gio.AppInfo.launch_default_for_uri(url, None)
