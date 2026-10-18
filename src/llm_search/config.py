"""Load user configuration from $LLM_SEARCH_HOME (default ~/.llm-search)."""

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
    """An app agent opened through a URL, e.g. Claude Desktop via claude://."""
    id: str
    name: str
    url: str                              # template: {prompt} {model} {effort}
    description: str = ""
    icon: str = "system-search-symbolic"
    tags: list[str] = field(default_factory=list)
    models: list[str] = field(default_factory=list)
    efforts: list[str] = field(default_factory=list)
    priority: float = 0.0
    copy_prompt: bool = False             # also copy the prompt to the clipboard


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
    secrets: dict[str, str] = field(default_factory=dict)


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


def load(directory: Path | None = None) -> Config:
    directory = directory or config_dir()
    try:
        with open(directory / "agents.toml", "rb") as f:
            data = tomllib.load(f)
    except FileNotFoundError:
        data = {}

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
        secrets=read_env_file(directory / "secrets.env"),
    )
