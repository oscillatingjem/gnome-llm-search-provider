"""Parser, ranker and URL tests. Run: python3 -m unittest discover -s tests"""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from llm_search import catalog, config, launch, query, ranker  # noqa: E402

EXAMPLE = config.load(ROOT / "examples" / "config")
# An app whose links accept a model and effort, to exercise those paths.
EXAMPLE.apps.append(config.App(id="agent", name="Agent", url="x://new?q={prompt}&model={model}&effort={effort}",
                               tags=["agent"], models=["opus", "sonnet"], efforts=["low", "high"]))
EXAMPLE.aliases["opus"] = "agent:opus"

GATEWAY_INFO = [
    {"model_name": "smart-router", "model_info": {"display_name": "Auto"}},
    {"model_name": "coder", "model_info": {"display_name": "Qwen Coder", "tags": ["coding", "private"],
                                           "aliases": ["coder"]}},
    {"model_name": "big@low", "model_info": {"base": "big", "effort": "low", "display_name": "Big",
                                             "tags": ["reasoning"]}},
    {"model_name": "big@high", "model_info": {"base": "big", "effort": "high"}},
]


def rows(text, cfg=EXAMPLE, gateway=False):
    cands = catalog.app_candidates(cfg)
    if gateway:
        cands = catalog.models_from_info(GATEWAY_INFO, cfg.auto_model) + cands
    return ranker.rank(query.parse(text.split()), cands, cfg)


class ParseTests(unittest.TestCase):
    CASES = [
        ("hello world", query.Query("hello world")),
        ("@opus !high refactor this", query.Query("refactor this", "opus", "high")),
        ("refactor !med this @son", query.Query("refactor this", "son", "medium")),
        ("!x", query.Query("", None, "max")),
        ("email bob@example.com now", query.Query("email bob@example.com now")),
        ("wow! !nope", query.Query("wow! !nope")),
        ("@ alone", query.Query("@ alone")),
    ]

    def test_cases(self):
        for text, expected in self.CASES:
            with self.subTest(text=text):
                self.assertEqual(query.parse(text.split()), expected)


class RankTests(unittest.TestCase):
    def ids(self, text, **kw):
        return [r.candidate.id for r in rows(text, **kw)]

    def test_short_or_token_only_queries_show_nothing(self):
        for text in ["hi", "@opus", "!high", "@opus !high"]:
            with self.subTest(text=text):
                self.assertEqual(rows(text), [])

    def test_coding_prompt_puts_claude_code_first(self):
        self.assertEqual(self.ids("fix this python traceback in my script")[0], "app:claude-code")

    def test_general_prompt_puts_ask_claude_first(self):
        self.assertEqual(self.ids("what should I cook tonight")[0], "app:claude-chat")

    def test_alias_with_model(self):
        top = rows("@opus refactor the snapshot script")[0]
        self.assertEqual((top.candidate.id, top.model), ("app:agent", "opus"))
        self.assertEqual(top.title, "Agent · Opus")

    def test_model_prefix_without_alias(self):
        top = rows("@son summarize this")[0]
        self.assertEqual((top.candidate.id, top.model), ("app:agent", "sonnet"))

    def test_app_alias(self):
        self.assertEqual(self.ids("@code what is a closure")[0], "app:claude-code")

    def test_apps_without_efforts_show_none(self):
        self.assertTrue(all(r.effort is None for r in rows("!high fix this bug") if r.candidate.id.startswith("app:claude")))

    def test_gateway_rows_include_auto_first(self):
        self.assertEqual(self.ids("explain btrfs snapshots", gateway=True)[0], "auto")

    def test_best_app_always_present(self):
        cfg = config.load(ROOT / "examples" / "config")
        cfg.max_rows = 2
        self.assertTrue(any(i.startswith("app:") for i in self.ids("explain regex", cfg=cfg, gateway=True)))

    def test_effort_forced_and_clamped(self):
        big = next(r for r in rows("!max compare these designs", gateway=True) if r.candidate.id == "model:big")
        self.assertEqual(big.effort, "high")          # max clamps to the highest supported
        low = next(r for r in rows("!low capital of peru", gateway=True) if r.candidate.id == "model:big")
        self.assertEqual(low.effort, "low")

    def test_effort_suggested(self):
        big = lambda t: next(r for r in rows(t, gateway=True) if r.candidate.id == "model:big").effort
        self.assertEqual(big("capital of peru"), "low")
        self.assertEqual(big("why does my btrfs balance keep failing on this disk"), "high")

    def test_models_without_efforts_show_none(self):
        coder = next(r for r in rows("@coder explain this regex", gateway=True))
        self.assertEqual((coder.candidate.id, coder.effort), ("model:coder", None))

    def test_result_ids_change_with_the_query(self):
        # Regression: GNOME caches rows by id, so identical ids froze the shown prompt.
        foo = {r.candidate.id: r.result_id for r in rows("foo")}
        foobar = {r.candidate.id: r.result_id for r in rows("foo bar")}
        self.assertTrue(foo and foo.keys() == foobar.keys())
        self.assertTrue(all(foo[k] != foobar[k] for k in foo))
        self.assertEqual(foo, {r.candidate.id: r.result_id for r in rows("foo")})   # stable for the same query
        self.assertEqual(len(set(foo.values())), len(foo))                         # unique within one search

    def test_result_id_round_trip(self):
        row = rows("@opus !high refactor this")[0]
        self.assertEqual(ranker.parse_result_id(row.result_id), ("app:agent", "opus", "high"))
        self.assertEqual(launch.url_for(EXAMPLE, *ranker.parse_result_id(row.result_id), "refactor this"),
                         "x://new?q=refactor%20this&model=opus&effort=high")


class CommandTests(unittest.TestCase):
    TEMPLATE = ["term", "--", "claude", ["--model", "{model}"], ["--effort", "{effort}"], "--", "{prompt}"]

    def test_full_command(self):
        self.assertEqual(launch.fill_command(self.TEMPLATE, prompt="fix it", model="opus", effort="high"),
                         ["term", "--", "claude", "--model", "opus", "--effort", "high", "--", "fix it"])

    def test_optional_groups_dropped(self):
        self.assertEqual(launch.fill_command(self.TEMPLATE, prompt="fix it", model=None, effort=None),
                         ["term", "--", "claude", "--", "fix it"])

    def test_prompt_is_one_argument_and_not_interpreted(self):
        argv = launch.fill_command(self.TEMPLATE, prompt="a; rm -rf ~ {model} $(x)", model="m", effort=None)
        self.assertEqual(argv[-1], "a; rm -rf ~ {model} $(x)")

    def test_missing_requirement_hides_app(self):
        cfg = config.load(ROOT / "examples" / "config")
        cfg.apps = [config.App(id="x", name="X", command=["x"], requires=["definitely-not-a-program-xyz"])]
        self.assertEqual(catalog.app_candidates(cfg), [])

    def test_example_cli_row_uses_model_and_effort(self):
        cfg = config.load(ROOT / "examples" / "config")
        app = next(a for a in cfg.apps if a.id == "claude-cli")
        argv, _ = launch.command_for(app, "opus", "xhigh", "refactor this")
        self.assertEqual(argv[-6:], ["--model", "opus", "--effort", "xhigh", "--", "refactor this"])

    def test_xhigh_effort_parses(self):
        self.assertEqual(query.parse(["!xh", "go"]).effort, "xhigh")


class UrlTests(unittest.TestCase):
    def test_app_url_encodes_prompt(self):
        url = launch.url_for(EXAMPLE, "app:claude-chat", None, None, "hello & goodbye?")
        self.assertEqual(url, "claude://claude.ai/new?q=hello%20%26%20goodbye%3F")

    def test_empty_params_dropped(self):
        url = launch.fill("x://new?q={prompt}&model={model}&effort={effort}", prompt="hi", model="opus", effort=None)
        self.assertEqual(url, "x://new?q=hi&model=opus")

    def test_gateway_urls(self):
        cfg = config.load(ROOT / "examples" / "config")
        cfg.chat_url = "http://127.0.0.1:3000/?models={model}&q={prompt}"
        self.assertEqual(launch.url_for(cfg, "model:big", None, "high", "hi"),
                         "http://127.0.0.1:3000/?models=big%40high&q=hi")
        self.assertEqual(launch.url_for(cfg, "auto", None, None, "hi"),
                         "http://127.0.0.1:3000/?models=smart-router&q=hi")

    def test_no_chat_url_means_no_gateway_launch(self):
        self.assertIsNone(launch.url_for(EXAMPLE, "model:big", None, None, "hi"))


if __name__ == "__main__":
    unittest.main()
