"""Record what the Python implementation does, as JSONL the Rust port must reproduce.

Run from the repo root:  python3 -I tests/golden/record.py
Writes tests/golden/*.jsonl. Programs are stubbed: everything is "installed" at
/usr/bin/<name> except names starting with "missing-", so results don't depend on the machine.
"""

import itertools
import json
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from llm_search import catalog, config, launch, query, ranker, toml_edit  # noqa: E402

launch.which = lambda p: None if p.startswith("missing-") else f"/usr/bin/{p}"
OUT = ROOT / "tests" / "golden"
EXAMPLE_DIR = ROOT / "examples" / "config"

GATEWAY_INFO = [
    {"model_name": "smart-router", "model_info": {"display_name": "Auto"}},
    {"model_name": "coder", "model_info": {"display_name": "Qwen Coder", "tags": ["coding", "private"],
                                           "aliases": ["Coder", "qwen"]}},
    {"model_name": "big@low", "model_info": {"base": "big", "effort": "low", "display_name": "Big",
                                             "tags": ["reasoning"]}},
    {"model_name": "big@high", "model_info": {"base": "big", "effort": "high"}},
    {"model_name": "plain"},
]


def configs():
    """(name, Config, include gateway models)"""
    base = config.load(EXAMPLE_DIR)
    yield "example", base, False

    extra = config.load(EXAMPLE_DIR)
    extra.apps.append(config.App(id="agent", name="Agent",
                                 url="x://new?q={prompt}&model={model}&effort={effort}",
                                 tags=["agent"], models=["opus", "sonnet"], efforts=["low", "high"]))
    extra.apps.append(config.App(id="gone", name="Gone", command=["missing-tool", "{prompt}"],
                                 requires=["missing-tool"]))
    extra.apps.append(config.App(id="off", name="Off", url="y://{prompt}", enabled=False))
    extra.aliases["opus"] = "agent:opus"
    extra.aliases["big"] = "model:big"
    extra.chat_url = "http://127.0.0.1:3000/?models={model}&q={prompt}"
    yield "extended", extra, True

    small = config.load(EXAMPLE_DIR)
    small.max_rows = 2
    small.min_chars = 1
    small.low_effort_max_words = 3
    yield "small", small, True


WORDS = ["fix", "this", "python", "traceback", "why", "does", "my", "btrfs", "balance", "fail",
         "write", "an", "email", "to", "bob@example.com", "capital", "of", "peru", "compare",
         "designs", "regex", "main()", "```", "private", "salary", "Éclair", "über", "naïve", "日本語",
         "wow!", "a", "hi", "ok?"]
TOKENS = ["@opus", "@son", "@fab", "@code", "@claude", "@cli", "@co", "@nope", "@agent", "@big", "@qwen",
          "@coder", "!low", "!l", "!med", "!m", "!high", "!h", "!xh", "!xhigh", "!max", "!x", "!bogus",
          "@", "!", "@OPUS", "!HIGH"]
FIXED = [
    "", "   ", "hi", "fix", "fix this python traceback in my script", "what should I cook tonight",
    "@opus refactor the snapshot script", "@son summarize this", "@code what is a closure",
    "!high fix this bug", "!low capital of peru", "!max compare these designs",
    "why does my btrfs balance keep failing on this disk",
    "explain btrfs snapshots", "@coder explain this regex", "email bob@example.com now", "wow! !nope",
    "@ alone", "!x", "@opus", "!high", "@opus !high",
    "a; rm -rf ~ {model} $(x) %s %(x)s",
    " ".join(["word"] * 70),
    "Ünïcödé prompt with ümlauts and 日本語",
    "trailing spaces   ", "tabs\tin\tterms",
]


def corpus():
    yield from FIXED
    for n, combo in enumerate(itertools.product(TOKENS[:14], WORDS[:12])):
        if n % 3 == 0:
            yield f"{combo[0]} {combo[1]} {' '.join(WORDS[(n % 20):(n % 20) + 4])}"
    for i in range(len(WORDS) - 3):
        yield " ".join(WORDS[i:i + 4])
        yield " ".join(WORDS[i:i + 9]) + f" {TOKENS[i % len(TOKENS)]}"


def record_ranking(out):
    for name, cfg, gateway in configs():
        cands = catalog.app_candidates(cfg)
        if gateway:
            cands = catalog.models_from_info(GATEWAY_INFO, cfg.auto_model) + cands
        for text in corpus():
            terms = text.split()
            q = query.parse(terms)
            rows = ranker.rank(q, cands, cfg)
            rec = {"config": name, "terms": terms,
                   "parsed": {"prompt": q.prompt, "model": q.model, "effort": q.effort},
                   "rows": []}
            for r in rows:
                cid, model, effort = ranker.parse_result_id(r.result_id)
                row = {"id": r.result_id, "title": r.title, "subtitle": r.subtitle,
                       "icon": r.candidate.icon}
                app = next((a for a in cfg.apps if f"app:{a.id}" == cid), None)
                if app and app.command:
                    try:
                        argv, cwd = launch.command_for(app, model, effort, q.prompt)
                        row["command"] = {"argv": argv, "cwd": cwd.replace(str(Path.home()), "~")}
                    except Exception as e:  # noqa: BLE001 - record the failure as data
                        row["error"] = type(e).__name__
                else:
                    try:
                        row["url"] = launch.url_for(cfg, cid, model, effort, q.prompt)
                    except Exception as e:  # noqa: BLE001
                        row["error"] = type(e).__name__
                rec["rows"].append(row)
            out.write(json.dumps(rec, ensure_ascii=False) + "\n")


def record_units(out):
    def w(kind, inp, result):
        out.write(json.dumps({"kind": kind, "in": inp, "out": result}, ensure_ascii=False) + "\n")

    cfg = config.load(EXAMPLE_DIR)
    for text in FIXED + [" ".join(WORDS[i:i + n]) for i in range(10) for n in (1, 3, 8, 9, 61)]:
        w("suggest_effort", text, ranker.suggest_effort(text, cfg))
    for effort in [None, "low", "medium", "high", "xhigh", "max"]:
        for supported in [[], ["low"], ["high"], ["low", "high"], ["medium"], ["low", "medium", "high", "xhigh", "max"]]:
            w("clamp_effort", [effort, supported], ranker.clamp_effort(effort, supported))
    for tmpl, vals in [
        ("x://new?q={prompt}&model={model}&effort={effort}", {"prompt": "hi", "model": "opus", "effort": None}),
        ("claude://claude.ai/new?q={prompt}", {"prompt": "hello & goodbye? 100% #1 ü/日"}),
        ("x://{prompt}", {"prompt": "a b/c"}),
        ("http://h/?models={model}&q={prompt}", {"model": "big@high", "prompt": "q=1&r=2"}),
        ("x://new?a=&b={prompt}&c=keep", {"prompt": ""}),
    ]:
        w("fill_url", [tmpl, vals], launch.fill(tmpl, **vals))
    tmpl = ["term", "--", "claude", ["--model", "{model}"], ["--effort", "{effort}"], "--", "{prompt}"]
    for vals in [{"prompt": "fix it", "model": "opus", "effort": "high"},
                 {"prompt": "fix it", "model": None, "effort": None},
                 {"prompt": "a; rm -rf ~ {model} $(x)", "model": "m", "effort": None},
                 {"prompt": "", "model": "m", "effort": "low"}]:
        w("fill_command", [tmpl, vals], launch.fill_command(tmpl, **vals))
    for rid in ["app:a||", "app:a|m|e|k", "model:big|high", "auto", "x|y|z|w|v", ""]:
        w("parse_result_id", rid, list(ranker.parse_result_id(rid)))
    for text in ["foo", "foo bar", "", "@opus !high x"]:
        q = query.parse(text.split())
        w("query_key", text, ranker.query_key(q))
    w("models_from_info", GATEWAY_INFO,
      [vars(c) for c in catalog.models_from_info(GATEWAY_INFO, "smart-router")])


def record_config(out):
    def w(kind, inp, result):
        out.write(json.dumps({"kind": kind, "in": inp, "out": result}, ensure_ascii=False) + "\n")

    c = config.load(EXAMPLE_DIR)
    summary = {k: v for k, v in vars(c).items() if k not in ("apps", "rules", "directory", "secrets")}
    summary["apps"] = [vars(a) for a in c.apps]
    summary["rules"] = [vars(r) for r in c.rules]
    w("load_example", "examples/config/agents.toml", summary)

    tmp = Path(tempfile.mkdtemp())
    env = tmp / "secrets.env"
    env.write_text("# comment\n\nKEY=value\n QUOTED = 'single' \nDQ=\"double\"\nnoequals\nEMPTY=\nA=b=c\n")
    w("read_env_file", env.read_text(), config.read_env_file(env))
    shutil.rmtree(tmp)

    original = (EXAMPLE_DIR / "agents.toml").read_text()
    edits = [
        [["search", "min_chars", None, 5]],
        [["search", "max_rows", None, 4], ["app", "priority", "claude-chat", 2.0]],
        [["prompt", "idle_minutes", None, 0]],
        [["prompt", "idle_minutes", None, 45.5]],
        [["routing", "low_effort_max_words", None, 20]],
        [["gateway", "url", None, "http://127.0.0.1:4000/#x"]],
        [["gateway", "url", None, "http://a"], ["gateway", "chat_url", None, "http://b/?q={prompt}"]],
        [["app", "enabled", "claude-cli", False], ["app", "cwd", "claude-cli", "~/Projects"]],
        [["app", "priority", "claude-cli", 2.5]],
        [["app", "cwd", "claude-chat", "~/ä \"quoted\" \\ back"]],
        [["app", "enabled", "no-such-agent", False]],
        [["app", "command", "claude-cli", "rm"]],
        [["search", "min_chars", None, True]],
    ]
    for edit in edits:
        try:
            result = {"text": toml_edit.apply(original, [tuple(e) for e in edit])}
        except toml_edit.EditError as e:
            result = {"error": str(e)}
        w("toml_apply", edit, result)


def main():
    with open(OUT / "ranking.jsonl", "w") as f:
        record_ranking(f)
    with open(OUT / "units.jsonl", "w") as f:
        record_units(f)
    with open(OUT / "config.jsonl", "w") as f:
        record_config(f)
    (OUT / "META").write_text(f"python {sys.version.split()[0]}\n")
    for p in sorted(OUT.glob("*.jsonl")):
        print(p.name, sum(1 for _ in open(p)))


if __name__ == "__main__":
    main()
