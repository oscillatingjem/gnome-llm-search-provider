"""Collect candidates: app agents from agents.toml plus models from the LiteLLM gateway."""

import json
import time
import urllib.request

from .config import Config
from .launch import app_available
from .ranker import APP, AUTO, MODEL, Candidate

CACHE_SECONDS = 60
RETRY_SECONDS = 30
TIMEOUT_SECONDS = 0.5


def app_candidates(config: Config) -> list[Candidate]:
    return [
        Candidate(id=f"app:{a.id}", kind=APP, name=a.name, tags=a.tags, efforts=a.efforts,
                  models=a.models, icon=a.icon, priority=a.priority)
        for a in config.apps if a.enabled and app_available(a)
    ]


def models_from_info(data: list[dict], auto_model: str) -> list[Candidate]:
    """Group LiteLLM /model/info entries (including generated effort variants) by base model."""
    by_base: dict[str, Candidate] = {}
    for entry in data:
        name = entry.get("model_name", "")
        info = entry.get("model_info") or {}
        if name == auto_model:
            by_base[name] = Candidate(id=AUTO, kind=AUTO, name=info.get("display_name", "Auto"),
                                      tags=info.get("tags", []), icon="emblem-system-symbolic")
            continue
        base = info.get("base") or name
        c = by_base.get(base)
        if c is None:
            c = by_base[base] = Candidate(
                id=f"{MODEL}:{base}", kind=MODEL, name=info.get("display_name", base),
                tags=info.get("tags", []), aliases=[a.lower() for a in info.get("aliases", [])],
                icon=info.get("icon", "user-available-symbolic"), priority=info.get("priority", 0.0))
        if info.get("effort") and info["effort"] not in c.efforts:
            c.efforts.append(info["effort"])
    return list(by_base.values())


class Gateway:
    """Reads the model list from LiteLLM, cached so searching stays fast."""

    def __init__(self, config: Config):
        self.config = config
        self._models: list[Candidate] = []
        self._fetched = 0.0
        self._failed = 0.0

    def available(self) -> bool:
        return bool(self.config.litellm_url)

    def models(self) -> list[Candidate]:
        if not self.available():
            return []
        now = time.monotonic()
        if now - self._fetched < CACHE_SECONDS or now - self._failed < RETRY_SECONDS:
            return self._models
        try:
            self._models = models_from_info(self._get("/model/info").get("data", []),
                                            self.config.auto_model)
            self._fetched = now
        except (OSError, ValueError):
            self._models, self._failed = [], now
        return self._models

    def _get(self, path: str) -> dict:
        req = urllib.request.Request(self.config.litellm_url + path)
        key = self.config.secrets.get(self.config.litellm_key_env)
        if key:
            req.add_header("Authorization", f"Bearer {key}")
        with urllib.request.urlopen(req, timeout=TIMEOUT_SECONDS) as resp:
            return json.load(resp)


def candidates(config: Config, gateway: Gateway) -> list[Candidate]:
    return gateway.models() + app_candidates(config)
