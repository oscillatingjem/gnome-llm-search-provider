"""Load and save user configuration in $LLM_SEARCH_HOME (default ~/.llm-search).

agents.toml is the single source of settings. You edit it by hand, and the settings window
edits the same file in place (toml_edit keeps its comments). Two things live elsewhere on
purpose: API keys in secrets.env (mode 600), and the keyboard shortcut in GNOME's settings.
"""

import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from . import toml_edit


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


AGENTS_FILE, SECRETS_FILE = "agents.toml", "secrets.env"
LEGACY_SETTINGS_FILE = "settings.toml"   # 0.4.0 kept window settings here; migrated on load


def _read_toml(path: Path) -> dict:
    try:
        with open(path, "rb") as f:
            return tomllib.load(f)
    except FileNotFoundError:
        return {}


def update(edits: list[tuple[str, str, str | None, object]], directory: Path | None = None) -> None:
    """Change values in agents.toml in place (see toml_edit.apply for the edit format).

    The file is rewritten atomically, and only if every edit verifies.
    """
    path = (directory or config_dir()) / AGENTS_FILE
    try:
        text = path.read_text()
    except FileNotFoundError:
        text = ""
    new = toml_edit.apply(text, edits)
    if new == text:
        return
    tmp = path.with_name(f".{path.name}.tmp")
    tmp.write_text(new)
    if path.exists():
        os.chmod(tmp, path.stat().st_mode & 0o777)
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
    """Changes whenever agents.toml or secrets.env changes; used to reload without restarting."""
    directory = directory or config_dir()
    out = []
    for name in (AGENTS_FILE, SECRETS_FILE):
        try:
            st = (directory / name).stat()
            out.append((st.st_mtime_ns, st.st_size))
        except FileNotFoundError:
            out.append(None)
    return tuple(out)


# General settings with factory values (Config's field defaults). A factory reset puts these
# back and leaves agents, aliases, routing rules and the gateway alone.
GENERAL = {
    ("search", "min_chars"): "min_chars",
    ("search", "max_rows"): "max_rows",
    ("prompt", "idle_minutes"): "prompt_idle_minutes",
    ("routing", "low_effort_max_words"): "low_effort_max_words",
}


def restore_defaults(directory: Path | None = None) -> None:
    factory = Config()
    update([(table, key, None, getattr(factory, attr)) for (table, key), attr in GENERAL.items()], directory)


def migrate_legacy_settings(directory: Path) -> None:
    """Fold a 0.4.0 settings.toml into agents.toml once, then remove it."""
    legacy = directory / LEGACY_SETTINGS_FILE
    data = _read_toml(legacy)
    if not legacy.exists():
        return
    edits = [(table, key, None, value)
             for table in ("search", "prompt", "routing", "gateway")
             for key, value in data.get(table, {}).items()]
    edits += [("app", key, app_id, value)
              for app_id, values in data.get("app", {}).items()
              for key, value in values.items() if key in ("enabled", "priority", "cwd")]
    try:
        update(edits, directory)
    except (toml_edit.EditError, OSError):
        return                                   # leave it in place rather than lose settings
    legacy.rename(legacy.with_name(LEGACY_SETTINGS_FILE + ".migrated"))


def load(directory: Path | None = None) -> Config:
    directory = directory or config_dir()
    migrate_legacy_settings(directory)
    data = _read_toml(directory / AGENTS_FILE)

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
