# Lean plan review (2026-10-18)

Two independent checkers reviewed `docs/RUST-MIGRATION.md`. All 27 findings were applied.

## 1. [breaks-user]

`llm-search rollback` is only described as "copy to engine/, keep engine.prev/", which cannot restore a working install in the two rollbacks the diagram promises. P2->P0: on the first switch engine.prev/ does not exist, because the previous engine was Python. P3->P2: engine.prev/ holds the 0.6 build, which has only the provider. If the AgentSearch .service is re-rendered to point into engine/, it points at an agent-search that no longer exists, so the shortcut, the app-grid launcher and LaunchSearch all fail. Also, running `install --engine=rust` a second time on the same version (for example to change --name or --shortcut) rotates engine/ into engine.prev/. That overwrites the last good build, and rollback then 'restores' the same broken build. Finally, the plan does not say rollback reloads the bus or stops the running Rust processes. agent-search stays resident, so after a rollback the old Rust process keeps the bus name.

**Fix applied:** Replace the Phase 2 installer bullets with: "The installer records the engine of each component (provider, prompt box: python or rust, plus the build version) in `state/install/engine.json`. It keeps the previous record as `engine.prev.json`, together with the matching rendered .service/.desktop files and `engine.prev/`. It rotates only when the version or engine changes; a reinstall of the same version leaves `engine.prev` alone. `llm-search rollback` restores the previous record. It re-renders each component's Exec line from it (Python Exec lines for a Python component, never a path into an engine dir that lacks that binary) and calls `org.freedesktop.DBus.ReloadConfig`. It then stops both bus names by PID and runs `llm-search status`. If any Exec target is missing, it refuses. `llm-search install --engine=python` is always a valid full rollback until 1.0."

## 2. [breaks-user]

Phase 5 deletes `bin/llm-search` and `src/llm_search` from the repo. Today `~/.local/bin/llm-search` (the shortcut's command) is a symlink to `<checkout>/bin/llm-search`. The Python .service Exec lines use `PYTHONPATH=<checkout>/src`. A user who runs `git pull` to 1.0 therefore loses the shortcut at once, through a dangling symlink. Python-engine users also lose GNOME search and the prompt box. By Decision 7 that is every user except the owner. All of this happens before `./install.sh` runs. If the 1.0 install.sh then can't build (no cargo 1.92, missing -devel packages, non-x86_64 with no prebuilt), the user has no working install and no Python to fall back on.

**Fix applied:** Add to Phase 5: "Upgrade safety: (1) 1.0 keeps `bin/llm-search` as a short POSIX-sh shim. If `$XDG_DATA_HOME/llm-search/engine/llm-search` exists, the shim execs it. Otherwise it prints 'Agent Search needs reinstalling: run ./install.sh' plus the requirement hint. This way the shortcut path never dangles. (2) install.sh checks requirements and builds or verifies the binary (`engine.new/llm-search --version`) before it touches any link or rendered file. On failure it exits with the existing install untouched and prints `git checkout v0.8.x && ./install.sh`. (3) The release notes and README say the upgrade is `git pull && ./install.sh`, run together, and that a Python-engine install stops working between the two. (4) The last 0.8.x release prints a warning about this from `llm-search status`."

## 3. [breaks-user]

The 1.0 rollback is not specified correctly (critique #3 applied to the lean plan's Phase 5). 1.0 still has an `llm-search rollback` command ("same commands"), but the plan does not say what it does there. If it restores engine.prev/ (the 0.8 build, whose `llm-search` has only the hidden search-provider subcommand) and `~/.local/bin/llm-search` points into engine/, the shortcut runs `llm-search prompt` against a binary without that verb and fails. The documented alternative, `git checkout v0.8.x && ./install.sh`, has its own problem. The 0.8 bash installer skips any target that is not a symlink. If the 1.0 Rust installer writes `~/.local/bin/llm-search`, the .service files or the .desktop file as regular files, the rollback quietly leaves them at 1.0.

**Fix applied:** Replace the Phase 5 Rollback with: "The Rust installer creates the same six targets as symlinks into `state/install/` and `engine/`, so the 0.8 bash installer can take them back. In 1.0, `llm-search rollback` does not swap to engine.prev. It prints the `git checkout v0.8.x && ./install.sh` instructions and exits non-zero. The Phase 5 exit requires a rehearsed `git checkout v0.8.x && ./install.sh` after which the shortcut, `llm-search prompt`, GNOME search and the settings window all work."

## 4. [missing]

Only Phase 2 requires a rollback rehearsal. Phases 3 and 5 have rollback arrows but no rehearsal in their exit criteria. Phase 4 (0.8.0) has no rollback path at all, in the diagram or the text, even though it is the release that first makes structural edits to agents.toml. The riskiest rollback (P3->P2, a mixed engine) is never exercised before it is needed.

**Fix applied:** Add to the Phase 3, 4 and 5 exits: "Rollback rehearsed. Afterwards the shortcut opens the prompt box, `llm-search query hello` returns rows, the prompt box opens its settings window, and the config backup can be restored." Add a Phase 4 Rollback: "`llm-search rollback` (back to the 0.7 engine), then optionally restore `state/backup/0.8.0-*`." Add a `P4 -. "llm-search rollback" .-> P3` arrow.

## 5. [missing]

The config backup appears only in the Phase 2 installer, where Rust doesn't write config. The plan never says a fresh backup is taken when 0.7.0 is installed (the first Rust writes, from the settings window) or 0.8.0 (structural edits). It also never says rollback offers to restore the backup. A user who edited config in the days between switches, and then has 0.7/0.8 mangle it, has a stale backup or none. They would also have to find it by hand.

**Fix applied:** Replace the Phase 2 bullet with: "Every `install` (all versions from 0.6.0 through 1.0) and every `rollback` copies `config/` to `state/backup/<version>-<time>/`, keeping modes, before changing anything. `llm-search rollback` lists the newest backup taken before the version being rolled back, and offers to restore it."

## 6. [missing]

Rust's loader rejects TOML 1.1-only syntax only "until Python is deleted", that is until 1.0. But 1.0's documented rollback goes to v0.8.x, whose Python and Rust loaders both reject TOML 1.1. Suppose a user hand-edits a 1.1 construct while on 1.0, such as a trailing comma or newline in an inline table, `\e`, or a time without seconds. 1.0 accepts it, but after `git checkout v0.8.x && ./install.sh`, search and the prompt box fail to load the config. Critique #15 said these rules should lift at 1.1, not 1.0.

**Fix applied:** Replace with: "Through 1.0.x, Rust rejects TOML 1.1-only syntax and writes TOML 1.0 only, so the v0.8.x rollback can always read the config. This rule lifts at 1.1."

## 7. [minor]

In Phase 4, the outside-edits banner offers "Keep editing". If saves then write the window's in-memory document, any change made on disk by a text editor is silently overwritten on the next instant save. Today's Python `config.update` re-reads the file from disk for every edit, so outside edits are never lost (this relates to critique #8: the editor model must match today's).

**Fix applied:** Add to Phase 4 Outside edits: "Every save re-reads the file and applies only the pending operation to the current on-disk text, as today. 'Keep editing' keeps the unsaved form contents but never writes back the window's old copy of the document. If the pending operation no longer applies, the save is refused with a toast."

## 8. [minor]

The new Exec lines are described only as pointing at binaries in engine/. Today's Exec lines pass `LLM_SEARCH_HOME` through `/usr/bin/env`, because a D-Bus-activated process does not inherit the user's shell environment. If that is dropped, a user with a custom LLM_SEARCH_HOME gets the default `~/.llm-search` in search and the prompt box. The D-Bus suite also does not say it activates through the rendered .service file, so an argv problem (critique #5) would only show up on the real desktop.

**Fix applied:** Add to §3 Changed: "Exec lines keep `/usr/bin/env LLM_SEARCH_HOME=@HOME@` in front of the binary." Add to the Phase 2 test: "The suite activates the provider through the rendered `.service` Exec line, with a non-default LLM_SEARCH_HOME."

## 9. [breaks-user]

The plan never says the new `.service` Exec lines still pass LLM_SEARCH_HOME, or that the prompt-box Exec still ends in `--gapplication-service`. It only says Exec points at `engine/`. Without LLM_SEARCH_HOME, anyone with a non-default config home loses their config once the provider is D-Bus-activated. Without the flag, D-Bus activation of the prompt box breaks.

**Fix applied:** In §3 Changed, replace the `.service` bullet with: "**`.service` Exec lines** become `Exec=/usr/bin/env LLM_SEARCH_HOME=@HOME@ <engine>/llm-search search-provider` and `Exec=/usr/bin/env LLM_SEARCH_HOME=@HOME@ <engine>/agent-search --gapplication-service`. PYTHONPATH and @PYTHON@ are dropped, so spawned commands no longer inherit PYTHONPATH. `agent-search` keeps the default `local_command_line`." Add a Phase 2 D-Bus test that activates the provider with a non-default LLM_SEARCH_HOME.

## 10. [breaks-user]

The plan's CLI list leaves out existing commands and options, and says nothing about `test`. `llm-search test` runs the Python unittest suite and disappears in 1.0.0. The comments in users' installed agents.toml point at `llm-search test` and `python3 -c "import tomllib…"`. Neither change is listed as a deviation.

**Fix applied:** Change the §2 diagram label to `llm-search install | uninstall | status | query | prompt/open | test | check | rollback | help`. In §3 Changed, add: "**`llm-search test`** in 1.0.0 runs the embedded config and pattern checks, the same as `check` (cargo test needs the repo). The example config's comments that mention `python3 -c \"import tomllib…\"` are rewritten to `llm-search check`." In Phase 5, add: "`uninstall [--purge] [--dry-run]`, the `open` alias and `--category-name` are kept."

## 11. [wrong-fact]

The plan hard-codes IS_SERVICE on the provider's gio::Application. The verified Rust facts explicitly say not to hard-code IS_SERVICE, because registration then fails when an instance is already running. They recommend relying on `--gapplication-service` instead. Python does use IS_SERVICE (DB-01), so the plan has to choose one or the other and say why.

**Fix applied:** Replace it with: "is a `gio::Application` with an inactivity timeout of 60 s. It is started as `llm-search search-provider --gapplication-service`; the subcommand is stripped before GLib parses argv, and IS_SERVICE is not hard-coded (rustFacts). Python 0.5.0 hard-codes it; the difference is recorded in deviations.md." Alternatively, keep IS_SERVICE and record in §6 Risks that this departs from the rustFacts guidance, with a test that a second activation exits cleanly.

## 12. [wrong-fact]

The plan relies on `llm-search check`, `llm-search search-provider` and `llm-search rollback` before the Rust CLI exists. Phase 1 builds only `core` and `platform`, yet its exit requires that the preflight (`llm-search check`) passes. Through Phases 2–4, `~/.llm-search`'s `llm-search` command is still the bash script, so `check` and `rollback` must be bash commands or forwarded to `engine/llm-search`. No phase says which.

**Fix applied:** Phase 1 Workspace becomes: "the `core`, `platform` and a minimal `cli` crate (`llm-search check`, plus `search-provider` stubbed)." In Phase 2 Installer, add: "The bash `llm-search` forwards `check` and `search-provider` to `engine/llm-search`, and implements `rollback` itself." Also say that after Python is deleted, `check` only compiles the patterns in Rust and no longer compares them with Python.

## 13. [missing]

The plan never says how the bash installer in Phases 2–4 gets the Rust binaries it copies to `engine/`. Only Phase 5 mentions prebuilt binaries or `cargo build --release --locked`. Also undefined: what `rollback` restores on the first switch, when `engine.prev/` is empty and the previous engine is Python, and whether it restores the config backup.

**Fix applied:** In Phase 2 Installer, add: "`--engine=rust` runs `cargo build --release --locked` from the repo, then copies `target/release/{llm-search,agent-search}` to `engine/`. `llm-search rollback` re-renders the two .service files for the engine recorded in `state/install/engine.prev` (`python` on the first switch), swaps `engine/` and `engine.prev/`, stops the old process by bus name, and prints the newest `state/backup/` path without restoring it."

## 14. [wrong-fact]

The Phase 5 rollback command uses a tag that no phase creates, and it would install the wrong engine. Only `v0.5.0` is ever tagged. Decision 7 makes Python the default engine before 1.0.0, so `./install.sh` from v0.8.x would bring back the Python provider and prompt box (which need PyGObject), not the Phase 4 Rust state the arrow points to.

**Fix applied:** In Phases 2, 3 and 4, add: "**Tag:** `v0.6.0` / `v0.7.0` / `v0.8.0`, after asking you before pushing." Change the Phase 5 rollback (and the diagram) to: `git checkout v0.8.0 && ./install.sh --engine=rust`.

## 15. [missing]

Phase 4 has no Exit criteria, no 'Installed at the end' line and no rollback path. Every other phase has all three. Phase 4 is also the first phase that writes structural config edits: it adds, deletes and renames agents and rules.

**Fix applied:** Add to Phase 4: "- **Installed at the end:** the full editor (0.8.0), with a config backup before the first structural edit. - **Rollback:** `llm-search rollback` (back to 0.7.0). - **Exit:** the tests pass, a manual checklist is done, and you've used it for 3–7 days." Add the arrow `P4 -. "llm-search rollback" .-> P3`. If you add the 3–7 days of use, update the header estimate to match.

## 16. [missing]

Phase 5 rewrites the installer and CLI in Rust but lists no tests. Several installer behaviors in the spec are never mentioned or tested: legacy-id cleanup (io.github.oscillatingjem.LlmSearchProvider, and the 0.1.0 ini link), the environment.d write and its safety check through the systemd generator, symlink handling, status/query output formats, dry-run output, and the uninstall/purge paths. The Phase 0 recordings cover only core logic, not installer or CLI output.

**Fix applied:** In Phase 0 Expected results, add: "plus the stdout, stderr, exit code and resulting file tree of `install --dry-run`, `install`, `install --copy --shortcut X`, `uninstall`, `uninstall --purge`, `status` and `query`, each run in a temp HOME with a fake gsettings and gdbus." Add a Phase 5 Tests bullet: "The Rust CLI reproduces those recordings, including legacy LlmSearchProvider cleanup and the environment.d check with its removal on failure."

## 17. [missing]

No phase ports or tests the gateway HTTP behavior. That covers models_from_info parsing (the auto model, base/effort variants, first-position order), the 60 s cache, the 30 s back-off, the Bearer header sent only when the secret is set, and the reset on reload. Reload-on-signature-change before every search (DB-07, SIG-01) and the legacy migration at provider start (DB-02) are also absent from the Phase 2 D-Bus suite.

**Fix applied:** In Phase 0 Expected results, add "models_from_info on recorded /model/info bodies". In the Phase 2 Test bullet, add: "a local mock /model/info server that checks the Authorization header, the 60 s cache, the 30 s back-off after an error or non-2xx reply, and the cache reset after a reload; editing agents.toml or secrets.env between two searches changes the results; a settings.toml present at provider start is migrated."

## 18. [missing]

The GNOME keyboard shortcut is never tested. That includes get/set/remove on the gsettings path, the list-ordering rules, name 'Agent Search' versus the installer's --name value, and the capture rules (modifier keys, Escape, BackSpace). Phase 3 tests only compare agents.toml.

**Fix applied:** In Phase 3 Tests, add: "Shortcut set/clear runs against GSETTINGS_BACKEND=memory with the g-s-d schemas: the path is appended to `custom-keybindings` after name/command/binding are written, removing it keeps the order of the other entries, `command` is `<HOME>/.local/bin/llm-search prompt`, and the capture rules match SW-23."

## 19. [missing]

The plan never mentions logging. Users and the docs grep the journal for lines prefixed 'llm-search: ' and for 'running <argv0> in <cwd>' and 'opening <url before ?>'. The rule that the prompt is never logged is not stated, and neither is the change from multi-line Python tracebacks.

**Fix applied:** Add to §3 Kept: "- stderr log lines `llm-search: running <argv0> in <cwd>` and `llm-search: opening <url without query>`; the prompt is never logged. Handler errors log `llm-search: <method> failed: <error>` on one line, with no Python traceback (listed in deviations.md)."

## 20. [missing]

§3 Kept lists LLM_SEARCH_HOME as the only environment variable. XDG_DATA_HOME, XDG_CONFIG_HOME, HOME and PATH are left out, as is the extra lookup in ~/.local/bin and ~/bin for `requires`. Retiring PYTHON and PYTHONPATH is not listed as a change either.

**Fix applied:** In §3 Kept, replace the `LLM_SEARCH_HOME` bullet with: "`LLM_SEARCH_HOME` (empty means `~/.llm-search`, no `~` expansion), `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `HOME`, and `PATH` plus `~/.local/bin` and `~/bin` for `requires`; `XDG_BIN_HOME` is still ignored." In §3 Changed, add: "`PYTHON` and `PYTHONPATH` are no longer read or set."

## 21. [missing]

Python's config load fails on unknown or missing keys in [[app]] and [[routing.rule]]. The plan only says wrongly typed values are accepted at load time and fail at ranking. It doesn't say whether unknown keys still fail the whole load, or whether ints are accepted where floats are expected (priority, weight, idle_minutes) and floats where ints are expected (min_chars). Strict serde would reject configs that load today.

**Fix applied:** Add to §2 Config loading: "`[[app]]` and `[[routing.rule]]` use `deny_unknown_fields` and required `id`/`name`/`tag`/`patterns`, so they fail at load like Python. Other tables ignore unknown keys. Numeric fields accept integer or float. Golden cases cover each one."

## 22. [missing]

The editing rules leave out three things the Phase 3 byte comparison will hit. First, value formatting: strings via json.dumps(ensure_ascii=True) give \uXXXX escapes, and floats use repr, so restore_defaults writes '30.0'. toml_edit's default quoting and number formatting differ. Second, every edit turns CRLF into LF and adds a final newline. Third, a missing table is appended with the marker '[<table>]  # added by the settings window'. Without explicit rules, Phase 3's comparison against Python's agents.toml output fails on non-ASCII values, on Restore Defaults, and on CRLF files.

**Fix applied:** Add to §2 Editing: "- values are written as TOML basic strings with Python's json.dumps escaping (non-ASCII becomes \\uXXXX; astral characters are written raw, which fixes Python's EditError and is listed in deviations.md); floats use Python repr (`30.0`, `1e+16`); - a missing table is appended as `[name]  # added by the settings window`; - line endings are normalized to LF with one final newline."

## 23. [minor]

Phase 4 replaces the settings window's automatic reload after an outside edit (silent reload plus the toast 'Updated from agents.toml') with a Reload/Keep editing banner. Rust Phase 3 shows TOML errors with line and column instead of tomllib's message text in toasts. Neither user-visible change is listed under §3 Changed, although the plan says every deviation is listed and approved.

**Fix applied:** Add to §3 Changed: "- **Outside edits (0.8.0):** a Reload / Keep editing banner replaces the automatic reload and its toast. - **Config error text:** toasts and D-Bus errors show the `toml` crate's message with line and column, not tomllib's."

## 24. [minor]

The plan doesn't say what `--copy` means once templates are embedded and the code lives in `engine/`. It also doesn't say whether a plain `uninstall` (without --purge) removes `engine/` and `engine.prev/`. Today a plain uninstall deletes only RENDER_DIR, so the Rust binaries would be left behind. `--purge` deletes the whole of $XDG_DATA_HOME/llm-search, rollback copies included.

**Fix applied:** Add to §3 Changed: "- From 1.0.0, `--copy` is accepted but does nothing, because the binary in `engine/` is always a copy. `uninstall` removes `engine/` and `engine.prev/`; `--purge` also removes `state/backup/`."

## 25. [minor]

The Phase 2 and Phase 3 tests need dbus-daemon (for gio TestDBus and dbus-run-session) and Xvfb with GSK_RENDERER=cairo. The Phase 0 toolchain command and the §5 build-package table leave them out.

**Fix applied:** Append `dbus-daemon xorg-x11-server-Xvfb` to the Phase 0 dnf command. Add a sentence under §5: "Running the tests also needs dbus-daemon and Xvfb (`xvfb-run`, `GSK_RENDERER=cairo`)."

## 26. [minor]

The plan names `blake2 0.10`† for result ids but doesn't require the variable-output API. Python's blake2s(digest_size=6) puts the length in the parameter block, so truncating a BLAKE2s-256 digest gives different ids. That would change every result id and break GNOME's metadata cache.

**Fix applied:** Change the crate bullet to: "`blake2 0.10`† using `Blake2sVar::new(6)` (digest length in the parameter block, not a truncated BLAKE2s-256), checked against the recorded Python ids on day 1 of Phase 1."

## 27. [minor]

The plan lists `llm-search prompt` with no text calling `Activate` as a change. Python 0.5.0 already does this: `gapplication launch` calls Activate, and gapplication already forwards the activation token. Only the Python fallback, used when gapplication is missing, really changes.

**Fix applied:** Change the bullet to: "**`llm-search prompt`** calls `Activate` (no text) or `ActivateAction('search')` (with text) directly over D-Bus, as `gapplication` does today, forwarding XDG_ACTIVATION_TOKEN and DESKTOP_STARTUP_ID. The fallback that ran without gapplication and ignored TEXT is removed."
