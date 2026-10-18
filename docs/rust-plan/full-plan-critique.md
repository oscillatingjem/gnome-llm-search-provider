**Completeness critique: Agent Search Rust migration plan**

The plan has 22 concrete problems. Each one below gives the problem, then replacement text for the plan.

**1. One `Action` cannot hold "copy, then launch" (L-01)**
Python copies the prompt to the clipboard first and then runs the command or opens the URL. The single-variant `Action {Copy, Spawn, OpenUri, Nothing}` cannot express that. It also cannot report the errors that must come back to D-Bus: an empty argv (IndexError), a missing cwd, or no URI handler. Each of these is a `Failed` reply today.
*Replace the §2 core row text with:* "Activation returns `Activation { copy: Option<String>, launch: Launch {Spawn{argv,cwd}, OpenUri(String), Nothing}, log: Option<String> }`, or `Err(ActivateError)` for an empty argv or an unknown placeholder. The executor runs the copy first and waits for wl-copy. It returns spawn or URI errors to the caller, so ActivateResult replies `Failed` exactly as Python does."

**2. `$XDG_DATA_HOME/llm-search/bin/` collides with `--copy` installs (CLI-06, CLI-10)**
In a `--copy` install, COPY_DIR is `$XDG_DATA_HOME/llm-search`, so `COPY_DIR/bin/llm-search` is the bash CLI that `~/.local/bin/llm-search` and the shortcut point to. Phase 2 would overwrite it with a Rust binary that has no CLI verbs yet, and `llm-search prompt` would break.
*Replace in §3 Changed and Phase 2:* "Rust binaries go to `$XDG_DATA_HOME/llm-search/engine/` (with `engine.prev/`), never to `bin/`, until Phase 4. Phase 4's legacy cleanup removes the stale `--copy` tree (`bin`, `src`, `share`, `examples`) under `$XDG_DATA_HOME/llm-search` only after the new `~/.local/bin/llm-search` link is in place."

**3. Phase 4 rollback to `bin.prev` breaks the shortcut**
The 0.7 `llm-search` binary only has the hidden `search-provider` mode. Rolling `~/.local/bin/llm-search` back to it breaks `llm-search prompt` and the desktop Exec line.
*Replace the Phase 4 Rollback:* "`llm-search rollback` restores `engine.prev` and re-points `~/.local/bin/llm-search` to the 0.7.x bash CLI. That CLI is kept as `state/install/llm-search.0.7.sh` until 1.0. Alternative: `git checkout v0.7.x && ./install.sh`. The Phase 4 exit requires `llm-search prompt` and the shortcut to work after a rehearsed rollback."

**4. "Stops both generations" will kill the installer itself**
The new process names contain `llm-search`. The bash installer and the CLI also run with comm and cmdline `llm-search`, so a `pkill -f llm-search` or `pkill -x llm-search` kills the installer and any running `llm-search prompt`.
*Replace in Phase 2 and §6:* "Running processes are stopped by bus name. For `io.github.oscillatingjem.AgentSearch.SearchProvider` and `io.github.oscillatingjem.AgentSearch`, call `org.freedesktop.DBus.GetConnectionUnixProcessID` and send SIGTERM to that PID. This covers the Python and Rust generations alike and never matches the installer. The old `pkill -f -- '-m llm_searc[h]'` stays as a fallback until 0.8."

**5. `llm-search search-provider` passes a positional argument to `GApplication::run`**
GApplication has no HANDLES_OPEN here. Its default `local_command_line` will refuse the extra argument ("This application can not open files") and exit, so GNOME search breaks.
*Add to §2 D-Bus:* "The `search-provider` subcommand is dispatched before GLib sees argv. The provider then calls `app.run_with_args(&[argv0])`, and any `--gapplication-*` options are forwarded unchanged. The TestDBus suite activates through the real rendered `.service` Exec line."

**6. `prompt` with no TEXT must call `Activate`, not `ActivateAction` (CLI-22, PB-05)**
`gapplication launch` calls `Activate`, which keeps the previous entry text. `ActivateAction('search', '')` would clear it.
*Replace in Phase 4:* "`prompt TEXT…` calls `ActivateAction('search', [<joined>], pd)`. `prompt` with no TEXT calls `org.freedesktop.Application.Activate(pd)`. `pd` carries `activation-token` from `XDG_ACTIVATION_TOKEN` and `desktop-startup-id` from `DESKTOP_STARTUP_ID`. Both use auto-start."

**7. The phase rule contradicts the plan's own scheduled changes**
- The §1 rule "No behavior fix ships in the same release as a switch" and the Phase rule ("anything that changes … file contents waits for Phase 5") conflict with several rows:
  - Phase 1 changes the writer bytes (UTF-8, CRLF kept) and starts rejecting URL template format specs.
  - The Phase 3 settings-window redesign ships in 0.7.0, the same release as the prompt-box switch.
  - The Phase 4 changes ship with the CLI switch: the `/usr/share` check, the "would remove" dry-run text, and Desktop Entry escaping.
- The Phase 3 proof (a byte comparison with the oracle) cannot pass if the writer bytes differ.

*Replace the §1 bullet:* "Apart from the Changed table, no behavior change ships with a switch. Every Changed row is a class B entry in `deviations.toml` and needs owner approval."
*Move these to Phase 5:* the writer-bytes and URL-template rows. Until Phase 5, Rust reproduces Python: `json.dumps` ensure_ascii escapes, CRLF→LF, and the `str.format` errors (KeyError, IndexError, ValueError reported as `Failed`).
*Add Decision 9:* "Settings window in 0.7.0: (a) a faithful port of the SW-01..25 layout, with the new structure in 1.0; or (b) the new structure in 0.7.0 as an approved class B change. Recommended: (a)."

**8. `edit::scalar` built on `toml_edit::DocumentMut` cannot reproduce today's editor**
It cannot reproduce:
- T-05: refusing multi-line values;
- T-02, T-04: fake headers inside arrays and strings, and the errors for quoted or dotted names;
- T-07: reverse order when inserting into a new table;
- T-10: line normalization;
- T-09: messages that embed the tomllib text.

*Replace the "Editing" paragraph:* "Until 1.0, `edit::scalar` is a line-for-line port of `toml_edit.py`. It uses the same HEADER and key regexes, `pycompat::splitlines`, the cut-point `_split_value`, and Python-repr messages, and it reparses with the Rust TOML parser. Messages that embed parser text are class B. `toml_edit::DocumentMut` is used only by `edit::structure` in Phase 5."

**9. TOML 1.1 input during mixed mode**
`toml` 1.1.8 accepts TOML 1.1 files that tomllib rejects. With the Rust provider and the Python prompt box running side by side, a hand-edited 1.1 file gives rows in GNOME search while the prompt box dies at load.
*Add to Config loading:* "Until Phase 4, the loader rejects TOML 1.1-only syntax (newlines or trailing commas in inline tables, `\e`, `\xHH`, times without seconds) with the tomllib-equivalent error, so both engines fail the same way. The corpus includes these cases."

**10. Strict serde types move failures from rank time to load time**
In Python, a value of the wrong type (for example `priority = "x"` or `tags = 3`) loads fine and only fails at rank time. With serde, `agent-search` would fail to start, which also takes away the settings window the user needs to fix the file.
*Add:* "Fields are deserialized as `toml::Value` and converted lazily. A type error is stored and raised where Python raises it: at rank time as D-Bus `Failed` or empty rows, with the settings window still usable. Only the TypeError cases (unknown or missing keys in `[[app]]` or `[[routing.rule]]`) fail the load."

**11. Gaps in the regex translator**
- `\S` and `\s` are not translated together, although the shipped `fast` rule uses `\S`.
- `&&`, `--` and `~~` inside a class are literals in Python but operators in Rust.
- A literal `{` or `a{3` is valid in Python but fails in Rust.
- Rust-only syntax is accepted: `\p{..}`, `\P{..}`, `(?<name>…)`, `\K`, `\G` and subroutine calls.

*Replace the translator list with:* "`\s`→`[\s\x1c-\x1f]`, `\S`→`[^\s\x1c-\x1f]` (and the same pair inside classes); escape `&`, `-` and `~` doubled inside classes; escape a `{` that does not start a valid quantifier. *Rejected list adds:* `\p`, `\P`, `(?<name>`, `\K`, `\G` and subroutine calls, each with the error Python gives."

**12. soup3 does not treat non-2xx responses as errors (G-03)**
urllib raises HTTPError on 4xx and 5xx, which sets `failed` and starts the 30 s back-off. soup3 would parse a 401 JSON body as `{}` and set `fetched`, giving a 60 s cache of `[]`.
*Add to the HTTP section:*
- "A status outside 200–299 counts as failure (`failed=now`)."
- "A non-object body or non-object entry replies D-Bus `Failed`, as Python's AttributeError does."
- "The body is decoded with `json.load`'s UTF-8/16/32 detection."
- "Each case gets a golden in the oracle's stubbed HTTP."

**13. No home for the GLib code both binaries share; installed binaries cannot find templates**
- `core` is GLib-free, but HTTP, the launch executor, ThemedIcon and gio::Settings are needed by both `llm-search` and `agent-search`.
- The `ui` row omits soup3.
- `llm-search install` run from an installed or prebuilt binary has no `share/` or `examples/` directory to read.

*Add a crate row:* "`platform` | lib | glib, gio, soup3 | HTTP gateway, launch executor, clipboard, ThemedIcon, shortcut (gio::Settings), process stop. Used by `cli` and `ui`."
*Add to §6:* "Templates, the `.ini`, the icon and `examples/config/*` are embedded with `include_str!` and `include_bytes!`."
*Add to Requirements:* glib-networking, needed for https gateways.

**14. The rollback spike only covers dbus-broker**
Debian 13 and Ubuntu use dbus-daemon.
*Replace:* "Rollback spike on Fedora 44 (dbus-broker) and Debian 13 or Ubuntu 24.04 (dbus-daemon). Re-run in the Phase 2 smoke matrix."

**15. There is no data rollback, and the 1.0 → 0.8 rollback breaks**
- No backup is taken before the first Rust write to `agents.toml` or `secrets.env`.
- If 1.0 writes new keys, the 0.8 loader (with `deny_unknown_fields`) cannot load them.

*Add to every switch phase:* "The installer copies `config/` to `state/backup/<version>-<timestamp>/` before switching, and `rollback` offers to restore it."
*Replace the Phase 5 rollback:* "The TOML-1.0 and no-new-keys rule lifts at 1.1, not 1.0. 0.8.x gains a forward-compatible loader that warns on unknown keys, and Phase 5 random sequences are also checked against the 0.8 Rust loader."

**16. The oracle interpreter is not pinned**
The spec was verified on Python 3.14.8 (Unicode 16.0). Python 3.11 rejects `\z` and has older Unicode tables.
*Add to Phase 0:* "The oracle and `gen_unicode_tables.py` run on one pinned Python (3.14.8, Unicode 16.0.0), recorded in `golden/META`. CI fails if the interpreter's version or `unicodedata.unidata_version` differs."

**17. The settings-window and shortcut behaviors are not mapped to tests**
- No Phase 3 proof names the SW IDs (SW-05 live refresh and its 300 ms toast, SW-14 restore close and reopen, SW-16 subtitles, SW-21 Test text, SW-23 capture) or SC-01..04.
- `agent-search` with `idle_minutes = 0` can quit before dconf writes are flushed.

*Add to the Phase 3 proof:* "A test per SW-01..25 and SC-01..04 ID. `gio::Settings::sync()` runs before every `quit()`. The settings window always writes the name 'Agent Search'; the installer writes `--name`."

**18. Async gateway in the prompt box can show stale rows**
*Add to Phase 3:* "Each `refresh()` carries a generation counter, and rows from a superseded keystroke are dropped."

**19. The oracle operation list is incomplete**
*Add operations:* `models_from_info`, `app_candidates`, `which`, `parse_result_id`, `signature`, and `update`. The `update` check covers the mode, the symlink replacement and the temp name `.agents.toml.tmp`.

**20. The estimates and soak lengths are inconsistent**
- Phase 0 (5–7 days) and Phase 1 (13–18 days) are low for the oracle with stubs, about 45 tests, the goldens and a full pycompat layer.
- The soak adds up to 4 weeks (7 + 7 + 14), not 3.
- The Phase 2 Changed row mentions a prebuilt tarball, which only exists from Phase 4.

*Replace:*
- "Phase 0: 8–10 days. Phase 1: 20–28 days."
- "Estimate: 75–103 focused days plus about 4 weeks of soak, roughly 5–8 calendar months part-time."
- Requirements row: "Phase 2–3: Rust 1.92+ to build; Phase 4: or the prebuilt tarball."

**21. Smaller corrections**
- The Phase 0 typelib check would block provider-only installs. *Replace:* "`check_requirements` warns (does not fail) when the Gtk 4 or Adw typelibs are missing."
- The facts do not confirm that NavigationView is libadwaita 1.4. *Add †.*
- *Add a † check:* "toml_edit's string encoder never emits TOML 1.1 escapes (`\e`, `\xHH`)."

**22. Missing owner decision on when Rust becomes the default**
*Add Decision 10:* "When Rust becomes the default for other users: at 0.7.0 for the provider and 0.8.0 for the prompt box, or opt-in until 0.8.0. Recommended: opt-in until 0.8.0."