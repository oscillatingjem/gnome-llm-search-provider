"""Parse a search query into model/effort tokens and the prompt text.

Tokens can appear anywhere in the query:
    @name   pick a model or agent by alias (prefix match)
    !level  pick an effort level (low, medium, high, xhigh, max)
Everything else is the prompt.
"""

from dataclasses import dataclass

EFFORTS = ("low", "medium", "high", "xhigh", "max")

EFFORT_ALIASES = {
    "l": "low", "lo": "low", "low": "low",
    "m": "medium", "med": "medium", "medium": "medium",
    "h": "high", "hi": "high", "high": "high",
    "xh": "xhigh", "xhigh": "xhigh",
    "x": "max", "max": "max",
}


@dataclass(frozen=True)
class Query:
    prompt: str
    model: str | None = None   # raw @alias text, lowercased, without the "@"
    effort: str | None = None  # one of EFFORTS

    @property
    def is_empty(self) -> bool:
        return not self.prompt


def parse(terms: list[str]) -> Query:
    """Parse GNOME's search terms (the query split on whitespace)."""
    words: list[str] = []
    model = effort = None
    for term in terms:
        if len(term) > 1 and term[0] == "@" and model is None:
            model = term[1:].lower()
        elif len(term) > 1 and term[0] == "!" and term[1:].lower() in EFFORT_ALIASES and effort is None:
            effort = EFFORT_ALIASES[term[1:].lower()]
        else:
            words.append(term)
    return Query(prompt=" ".join(words).strip(), model=model, effort=effort)
