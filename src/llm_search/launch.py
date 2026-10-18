"""Build the URL for a chosen row and open it."""

import shutil
import subprocess
from urllib.parse import parse_qsl, quote, urlencode, urlsplit, urlunsplit

from .config import Config
from .ranker import APP, AUTO, MODEL


def fill(template: str, **values: str | None) -> str:
    """Fill {placeholders} (URL-encoded), then drop query parameters that ended up empty."""
    url = template.format(**{k: quote(v or "", safe="") for k, v in values.items()})
    parts = urlsplit(url)
    if not parts.query:
        return url
    query = [(k, v) for k, v in parse_qsl(parts.query, keep_blank_values=True) if v]
    return urlunsplit(parts._replace(query=urlencode(query, quote_via=quote)))


def gateway_model_id(base: str, effort: str | None) -> str:
    return f"{base}@{effort}" if effort else base


def url_for(config: Config, cid: str, model: str | None, effort: str | None, prompt: str) -> str | None:
    kind, _, name = cid.partition(":")
    if kind == APP:
        app = next((a for a in config.apps if a.id == name), None)
        return app and fill(app.url, prompt=prompt, model=model, effort=effort)
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
