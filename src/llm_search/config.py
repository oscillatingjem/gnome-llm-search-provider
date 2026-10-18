"""Load user configuration from $LLM_SEARCH_HOME (default ~/.llm-search).

Two files make up the configuration:
  agents.toml    written by hand (commented); agents, aliases, routing rules
  settings.toml  written by the settings window; overrides agents.toml, so the
                 window never has to rewrite (and lose the comments of) agents.toml
"""

import json
import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path


def home() -> Path:
    return Path(os.environ.get("LLM_SEARCH_HOME") or Path.home() / ".llm-search")


def config_dir() -> Path:
    return home() / "config"


def state_dir() -> Path:
    return home() / "state"


@dataclass
class App:
    """An app agent, opened through a URL (e.g. claude://) or by running a command."""
    id: str
    name: str
    url: str = ""                         # template: {prompt} {model} {effort}
    command: list = field(default_factory=list)   # argv template; nested lists are optional groups
    cwd: str = "~"                        # working directory for `command`
    requires: list[str] = field(default_factory=list)  # programs that must exist; hidden otherwise
    description: str = ""
    icon: str = "system-search-symbolic"
    tags: list[str] = field(default_factory=list)
    models: list[str] = field(default_factory=list)
    efforts: list[str] = field(default_factory=list)
    priority: float = 0.0
    copy_prompt: bool = False             # also copy the prompt to the clipboard
    enabled: bool = True                  # false hides the row


@dataclass
class Rule:
    """Adds `weight` to candidates with `tag` when any pattern matches the prompt."""
    tag: str
    patterns: list[str]
    weight: float = 1.0


@dataclass
class Config:
    min_chars: int = 3
    max_rows: int = 5
    apps: list[App] = field(default_factory=list)
    aliases: dict[str, str] = field(default_factory=dict)
    rules: list[Rule] = field(default_factory=list)
    high_effort_patterns: list[str] = field(default_factory=list)
    low_effort_max_words: int = 8
    litellm_url: str = ""                 # empty = gateway disabled
    litellm_key_env: str = "LITELLM_MASTER_KEY"
    chat_url: str = ""                    # template: {model} {prompt}
    auto_model: str = "smart-router"
    prompt_idle_minutes: float = 30.0     # prompt box stays running this long after use; 0 = quit
    secrets: dict[str, str] = field(default_factory=dict)
    directory: Path | None = None         # where this config was loaded from


def read_env_file(path: Path) -> dict[str, str]:
    """Parse a simple KEY=value file (comments and blank lines ignored)."""
    values: dict[str, str] = {}
    try:
        text = path.read_text()
    except OSError:
        return values
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        values[key.strip()] = value.strip().strip("'\"")
    return values


AGENTS_FILE, SETTINGS_FILE, SECRETS_FILE = "agents.toml", "settings.toml", "secrets.env"
# Keys the settings window may override, per section of agents.toml.
OVERRIDABLE = {
    "search": {"min_chars", "max_rows"},
    "prompt": {"idle_minutes"},
    "routing": {"low_effort_max_words"},
    "gateway": {"url", "key_env", "chat_url", "auto_model"},
}
APP_OVERRIDABLE = {"enabled", "priority", "cwd"}


def _read_toml(path: Path) -> dict:
    try:
        with open(path, "rb") as f:
            return tomllib.load(f)
    except FileNotFoundError:
        return {}


def read_settings(directory: Path | None = None) -> dict:
    return _read_toml((directory or config_dir()) / SETTINGS_FILE)


def _toml_value(value) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float)):
        return repr(value)
    return json.dumps(str(value))       # a JSON string is a valid TOML basic string


def write_settings(data: dict, directory: Path | None = None) -> None:
    """Write settings.toml: [section] tables of scalars, plus [app.<id>] tables."""
    lines = ["# Written by the Agent Search settings window. These values override agents.toml.",
             "# Delete a line (or this file) to go back to what agents.toml says.", ""]
    for section, values in data.items():
        if section == "app":
            continue
        if values:
            lines += [f"[{section}]", *(f"{k} = {_toml_value(v)}" for k, v in values.items()), ""]
    for app_id, values in data.get("app", {}).items():
        if values:
            lines += [f"[app.{json.dumps(app_id)}]", *(f"{k} = {_toml_value(v)}" for k, v in values.items()), ""]
    path = (directory or config_dir()) / SETTINGS_FILE
    tmp = path.with_suffix(".tmp")
    tmp.write_text("\n".join(lines))
    tmp.replace(path)


def set_secret(key: str, value: str, directory: Path | None = None) -> None:
    """Set (or, with an empty value, remove) KEY in secrets.env, keeping other lines and mode 600."""
    path = (directory or config_dir()) / SECRETS_FILE
    try:
        lines = path.read_text().splitlines()
    except FileNotFoundError:
        lines = []
    kept = [ln for ln in lines if ln.strip().partition("=")[0].strip() != key]
    if value:
        kept.append(f"{key}={value}")
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write("\n".join(kept) + "\n")
    os.chmod(path, 0o600)


def signature(directory: Path | None = None) -> tuple:
    """Changes whenever any config file changes; used to reload without restarting."""
    directory = directory or config_dir()
    out = []
    for name in (AGENTS_FILE, SETTINGS_FILE, SECRETS_FILE):
        try:
            st = (directory / name).stat()
            out.append((st.st_mtime_ns, st.st_size))
        except FileNotFoundError:
            out.append(None)
    return tuple(out)


def _merge(data: dict, overrides: dict) -> dict:
    for section, keys in OVERRIDABLE.items():
        values = {k: v for k, v in overrides.get(section, {}).items() if k in keys}
        if values:
            data.setdefault(section, {}).update(values)
    app_overrides = overrides.get("app", {})
    for app in data.get("app", []):
        values = {k: v for k, v in app_overrides.get(app.get("id"), {}).items() if k in APP_OVERRIDABLE}
        app.update(values)
    return data


def load(directory: Path | None = None) -> Config:
    directory = directory or config_dir()
    data = _merge(_read_toml(directory / AGENTS_FILE), _read_toml(directory / SETTINGS_FILE))

    search = data.get("search", {})
    routing = data.get("routing", {})
    gateway = data.get("gateway", {})
    return Config(
        min_chars=search.get("min_chars", 3),
        max_rows=search.get("max_rows", 5),
        apps=[App(**a) for a in data.get("app", [])],
        aliases={k.lower(): v for k, v in data.get("aliases", {}).items()},
        rules=[Rule(**r) for r in routing.get("rule", [])],
        high_effort_patterns=routing.get("high_effort_patterns", []),
        low_effort_max_words=routing.get("low_effort_max_words", 8),
        litellm_url=gateway.get("url", "").rstrip("/"),
        litellm_key_env=gateway.get("key_env", "LITELLM_MASTER_KEY"),
        chat_url=gateway.get("chat_url", ""),
        auto_model=gateway.get("auto_model", "smart-router"),
        prompt_idle_minutes=data.get("prompt", {}).get("idle_minutes", 30.0),
        secrets=read_env_file(directory / SECRETS_FILE),
        directory=directory,
    )
