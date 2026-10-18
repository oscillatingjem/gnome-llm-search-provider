"""Turn a parsed query and the available agents/models into ranked search rows."""

import re
from dataclasses import dataclass, field

from .config import Config
from .query import EFFORTS, Query

APP, MODEL, AUTO = "app", "model", "auto"


@dataclass
class Candidate:
    """Something a prompt can be sent to: an app agent, a gateway model, or the Auto router."""
    id: str                       # "app:<id>", "model:<base id>", "auto"
    kind: str
    name: str
    tags: list[str] = field(default_factory=list)
    efforts: list[str] = field(default_factory=list)
    models: list[str] = field(default_factory=list)   # app agents only
    aliases: list[str] = field(default_factory=list)
    icon: str = "system-search-symbolic"
    priority: float = 0.0


@dataclass
class Row:
    candidate: Candidate
    model: str | None = None      # app agents: chosen model
    effort: str | None = None
    reasons: list[str] = field(default_factory=list)

    @property
    def result_id(self) -> str:
        return f"{self.candidate.id}|{self.model or ''}|{self.effort or ''}"

    @property
    def title(self) -> str:
        parts = [self.candidate.name]
        if self.model:
            parts.append(self.model.capitalize())
        return " · ".join(parts)

    @property
    def subtitle(self) -> str:
        parts = []
        if self.effort:
            parts.append(f"{self.effort} effort")
        if self.reasons:
            parts.append(", ".join(self.reasons))
        return " · ".join(parts)


def parse_result_id(result_id: str) -> tuple[str, str | None, str | None]:
    cid, model, effort = (result_id.split("|") + ["", ""])[:3]
    return cid, model or None, effort or None


def suggest_effort(prompt: str, config: Config) -> str:
    if any(re.search(p, prompt, re.IGNORECASE) for p in config.high_effort_patterns):
        return "high"
    words = len(prompt.split())
    if words <= config.low_effort_max_words:
        return "low"
    return "high" if words > 60 else "medium"


def clamp_effort(effort: str | None, supported: list[str]) -> str | None:
    """Pick the supported effort closest to the requested one."""
    if not effort or not supported:
        return None
    if effort in supported:
        return effort
    want = EFFORTS.index(effort)
    return min(supported, key=lambda e: abs(EFFORTS.index(e) - want) if e in EFFORTS else 99)


def matched_tags(prompt: str, config: Config) -> dict[str, float]:
    weights: dict[str, float] = {}
    for rule in config.rules:
        if any(re.search(p, prompt, re.IGNORECASE) for p in rule.patterns):
            weights[rule.tag] = weights.get(rule.tag, 0.0) + rule.weight
    return weights


def resolve(name: str, candidates: list[Candidate], config: Config) -> tuple[Candidate, str | None] | None:
    """Resolve an @alias to a candidate (and, for app agents, one of its models)."""
    target = config.aliases.get(name)
    if target:
        cid, _, model = target.partition(":")
        for c in candidates:
            if c.id in (cid, f"app:{cid}", f"model:{cid}"):
                return c, model or None
    for c in candidates:                              # exact, then prefix, matches
        keys = [c.id.split(":", 1)[-1], c.name.lower(), *c.aliases]
        if name in keys:
            return c, None
    for c in candidates:
        if c.kind == APP and any(m.startswith(name) for m in c.models):
            return c, next(m for m in c.models if m.startswith(name))
    for c in candidates:
        keys = [c.id.split(":", 1)[-1], c.name.lower(), *c.aliases]
        if any(k.startswith(name) for k in keys):
            return c, None
    return None


def rank(query: Query, candidates: list[Candidate], config: Config) -> list[Row]:
    if len(query.prompt) < config.min_chars:
        return []

    effort = query.effort or suggest_effort(query.prompt, config)
    tags = matched_tags(query.prompt, config)

    def make_row(c: Candidate, model: str | None = None) -> Row:
        # With no @model, app agents keep their own default model.
        return Row(c, model, clamp_effort(effort, c.efforts), [t for t in tags if t in c.tags])

    rows: list[Row] = []
    if query.model:
        hit = resolve(query.model, candidates, config)
        if hit:
            rows.append(make_row(*hit))

    def score(c: Candidate) -> float:
        return c.priority + sum(w for t, w in tags.items() if t in c.tags)

    ranked = sorted((c for c in candidates if c.kind != AUTO), key=score, reverse=True)
    auto = [c for c in candidates if c.kind == AUTO]

    for c in auto + ranked:
        if len(rows) >= config.max_rows:
            break
        if not any(r.candidate.id == c.id for r in rows):
            rows.append(make_row(c))

    # Always keep the best app agent visible, even when many gateway models outrank it.
    best_app = next((c for c in ranked if c.kind == APP), None)
    if best_app and not any(r.candidate.kind == APP for r in rows):
        rows[-1:] = [make_row(best_app)]
    return rows
