"""Parser, ranker and URL tests. Run: python3 -m unittest discover -s tests"""

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "src"))

from llm_search import catalog, config, launch, query, ranker, toml_edit  # noqa: E402

# Results must not depend on what's installed on the test machine: treat every program
# as installed except ones named "missing-...".
launch.which = lambda program: None if program.startswith("missing-") else f"/usr/bin/{program}"

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
        # The terminal row's priority puts it above the desktop Claude Code row.
        self.assertEqual(self.ids("fix this python traceback in my script")[:2], ["app:claude-cli", "app:claude-code"])

    def test_general_prompt_puts_ask_claude_first(self):
        self.assertEqual(self.ids("what should I cook tonight")[0], "app:claude-chat")

    def test_alias_with_model(self):
        top = rows("@opus refactor the snapshot script")[0]
        self.assertEqual((top.candidate.id, top.model), ("app:agent", "opus"))
        self.assertEqual(top.title, "Agent · Opus")

    def test_model_prefix_without_alias(self):
        top = rows("@fab summarize this")[0]          # only the terminal row lists "fable"
        self.assertEqual((top.candidate.id, top.model), ("app:claude-cli", "fable"))

    def test_app_alias(self):
        self.assertEqual(self.ids("@code what is a closure")[0], "app:claude-code")

    def test_apps_without_efforts_show_none(self):
        desktop = [r for r in rows("!high fix this bug") if r.candidate.id in ("app:claude-chat", "app:claude-code")]
        self.assertEqual(len(desktop), 2)
        self.assertTrue(all(r.effort is None for r in desktop))

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
        cfg.apps = [config.App(id="x", name="X", command=["x"], requires=["missing-program"])]
        self.assertEqual(catalog.app_candidates(cfg), [])

    def test_cli_row_hidden_without_claude(self):
        cfg = config.load(ROOT / "examples" / "config")
        for app in cfg.apps:
            app.requires = ["missing-" + r for r in app.requires]
        self.assertNotIn("app:claude-cli", [c.id for c in catalog.app_candidates(cfg)])

    def test_example_cli_row_uses_model_and_effort(self):
        cfg = config.load(ROOT / "examples" / "config")
        app = next(a for a in cfg.apps if a.id == "claude-cli")
        argv, _ = launch.command_for(app, "opus", "xhigh", "refactor this")
        self.assertEqual(argv, ["/usr/bin/ptyxis", "--new-window", "--", "/usr/bin/claude",
                                "--model", "opus", "--effort", "xhigh", "--", "refactor this"])

    def test_xhigh_effort_parses(self):
        self.assertEqual(query.parse(["!xh", "go"]).effort, "xhigh")


class SettingsTests(unittest.TestCase):
    """The settings window edits agents.toml itself: one source of settings, comments kept."""

    def setUp(self):
        import shutil, tempfile
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        shutil.copy(ROOT / "examples" / "config" / "agents.toml", self.dir / "agents.toml")
        self.original = (self.dir / "agents.toml").read_text()

    def text(self):
        return (self.dir / "agents.toml").read_text()

    def test_edits_apply_and_keep_every_comment(self):
        config.update([("search", "min_chars", None, 5), ("prompt", "idle_minutes", None, 0),
                       ("app", "enabled", "claude-cli", False), ("app", "cwd", "claude-cli", "~/Projects"),
                       ("app", "priority", "claude-cli", 2.5)], self.dir)
        c = config.load(self.dir)
        cli = next(a for a in c.apps if a.id == "claude-cli")
        self.assertEqual((c.min_chars, c.prompt_idle_minutes), (5, 0))
        self.assertEqual((cli.enabled, cli.cwd, cli.priority), (False, "~/Projects", 2.5))
        self.assertNotIn("app:claude-cli", [x.id for x in catalog.app_candidates(c)])
        comments = lambda s: [ln for ln in s.splitlines() if ln.lstrip().startswith("#")]
        self.assertEqual(comments(self.text()), comments(self.original))
        self.assertIn('priority = 2.5        # ranks above the desktop "Claude Code" row', self.text())

    def test_only_the_edited_lines_change(self):
        import difflib
        config.update([("search", "max_rows", None, 4), ("app", "priority", "claude-chat", 2.0)], self.dir)
        changed = [d for d in difflib.ndiff(self.original.splitlines(), self.text().splitlines())
                   if d[:1] in "+-"]
        self.assertEqual(changed, ["- max_rows = 5", "+ max_rows = 4",
                                   "- priority = 1.0        # the default choice for everyday questions",
                                   "+ priority = 2.0        # the default choice for everyday questions"])

    def test_missing_gateway_table_is_added(self):
        config.update([("gateway", "url", None, "http://127.0.0.1:4000/#x")], self.dir)
        self.assertEqual(config.load(self.dir).litellm_url, "http://127.0.0.1:4000/#x")

    def test_unknown_agent_is_refused_and_file_untouched(self):
        with self.assertRaises(toml_edit.EditError):
            config.update([("app", "enabled", "no-such-agent", False)], self.dir)
        self.assertEqual(self.text(), self.original)

    def test_multiline_values_are_refused(self):
        with self.assertRaises(toml_edit.EditError):
            config.update([("app", "command", "claude-cli", "rm")], self.dir)
        self.assertEqual(self.text(), self.original)

    def test_restore_defaults_resets_general_but_keeps_agents_rules_and_gateway(self):
        config.update([("search", "min_chars", None, 9), ("search", "max_rows", None, 2),
                       ("prompt", "idle_minutes", None, 0), ("routing", "low_effort_max_words", None, 20),
                       ("gateway", "url", None, "http://127.0.0.1:4000"),
                       ("app", "enabled", "claude-cli", False), ("app", "cwd", "claude-cli", "~/work")], self.dir)
        config.restore_defaults(self.dir)
        c, factory = config.load(self.dir), config.Config()
        self.assertEqual((c.min_chars, c.max_rows, c.prompt_idle_minutes, c.low_effort_max_words),
                         (factory.min_chars, factory.max_rows, factory.prompt_idle_minutes, factory.low_effort_max_words))
        cli = next(a for a in c.apps if a.id == "claude-cli")
        self.assertEqual((cli.enabled, cli.cwd), (False, "~/work"))         # agent settings kept
        self.assertEqual(c.litellm_url, "http://127.0.0.1:4000")             # gateway kept
        self.assertEqual((len(c.apps), len(c.rules)), (3, 5))                # agents and rules kept

    def test_legacy_settings_file_is_migrated_once(self):
        (self.dir / "settings.toml").write_text('[search]\nmax_rows = 3\n[app."claude-cli"]\ncwd = "~/x"\n')
        c = config.load(self.dir)
        self.assertEqual((c.max_rows, next(a.cwd for a in c.apps if a.id == "claude-cli")), (3, "~/x"))
        self.assertFalse((self.dir / "settings.toml").exists())
        self.assertIn("max_rows = 3", self.text())

    def test_signature_changes_on_update(self):
        before = config.signature(self.dir)
        config.update([("search", "max_rows", None, 3)], self.dir)
        self.assertNotEqual(before, config.signature(self.dir))

    def test_set_secret_keeps_other_lines_and_mode(self):
        path = self.dir / "secrets.env"
        path.write_text("# comment\nOTHER=1\nLITELLM_MASTER_KEY=old\n")
        config.set_secret("LITELLM_MASTER_KEY", "new", self.dir)
        self.assertEqual(path.read_text(), "# comment\nOTHER=1\nLITELLM_MASTER_KEY=new\n")
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        config.set_secret("LITELLM_MASTER_KEY", "", self.dir)
        self.assertNotIn("LITELLM_MASTER_KEY", path.read_text())


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
