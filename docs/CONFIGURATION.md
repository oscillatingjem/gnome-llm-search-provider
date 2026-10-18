# Configuration

## Where config lives
`$LLM_SEARCH_HOME`, default `~/.llm-search`:

```
~/.llm-search/
  config/
    agents.toml      what you edit
    secrets.env      KEY=value, mode 600
  state/
    install/         rendered service and desktop files (the installer recreates these)
```

### One source of settings
`agents.toml` is the only settings file. The settings window (gear on the prompt box) edits the same
file you do: it changes just the value on the line in question, keeps every comment, adds a key below
its table header when the key is missing, and checks the result before saving. So the file and the
window always agree, and you can switch between them freely. If you edit the file while the window is
open, the window refreshes.

What the window can change:

| Table | Keys |
|---|---|
| `[search]` | `min_chars`, `max_rows` |
| `[prompt]` | `idle_minutes` |
| `[routing]` | `low_effort_max_words` |
| `[gateway]` | `url`, `chat_url`, `auto_model` |
| each `[[app]]` | `enabled`, `priority`, `cwd` |

Everything else (agents, commands, aliases, ranking rules) is edited in the file.

Two things are deliberately kept elsewhere, and the window shows and edits both:
- **API keys** go in `secrets.env` (mode 600), so secrets never sit in a file you might share.
- **The keyboard shortcut** is a GNOME custom shortcut, because GNOME reads shortcuts from its own
  settings. It also appears in Settings → Keyboard → Custom Shortcuts.

**Restore Defaults** (General page) puts the table above's `[search]`, `[prompt]` and `[routing]`
values back to factory defaults in `agents.toml`. Agents, aliases, rules, the gateway and the shortcut
are kept.

Upgrading from 0.4.0, which kept window settings in a separate `settings.toml`: it's merged into
`agents.toml` automatically and renamed to `settings.toml.migrated`.

`LLM_SEARCH_HOME` is read when you run `install.sh`, and the installer writes it into the D-Bus service
file. To move your config, re-run the installer with the new value.

Both the search provider and the prompt box notice when any of the three files changes, and reload it
on the next search or the next time the box opens.

## `agents.toml`

### `[search]`
| Key | Default | Meaning |
|---|---|---|
| `min_chars` | `3` | Prompts shorter than this show no rows |
| `max_rows` | `5` | Rows to return (GNOME shows about five) |

### `[[app]]`: app agents
| Key | Required | Meaning |
|---|---|---|
| `id` | yes | Unique id; also matches `@id` |
| `name` | yes | Row title |
| `url` | one of `url`/`command` | Link to open. Placeholders: `{prompt}`, `{model}`, `{effort}`, all URL-encoded. Query parameters that end up empty are dropped. |
| `command` | one of `url`/`command` | Program to run, as an argv list; it's never passed through a shell, so the prompt is always a single argument. Same placeholders. An item whose placeholder is empty is dropped, and a nested list is an optional group dropped as a whole, e.g. `["--model", "{model}"]`. |
| `cwd` | | Working directory for `command` (default `~`) |
| `requires` | | Programs that must exist (searched on `PATH` plus `~/.local/bin` and `~/bin`). The row is hidden if any is missing, and matching argv items are replaced with their full path. |
| `icon` | | Icon name from your icon theme |
| `tags` | | Capability tags for ranking (`coding`, `writing`, `reasoning`, `private`, `fast`, …) |
| `models` | | Models the link can choose (`{model}`); lets `@model` pick one |
| `efforts` | | Effort levels the link accepts (`{effort}`) |
| `priority` | | Added to the ranking score (default 0) |
| `copy_prompt` | | Also copy the prompt to the clipboard (for apps whose links can't carry it) |
| `enabled` | | `false` hides the row (default `true`); also a switch in the settings window |

### `[aliases]`
`name = "target"`, where the target is `"<app id>"`, `"<app id>:<model>"` or `"<gateway model>"`.
`@name` checks aliases first, then exact ids and names, then app model names, then prefixes.

### `[gateway]`: optional
| Key | Default | Meaning |
|---|---|---|
| `url` | (off) | LiteLLM base URL, e.g. `http://127.0.0.1:4000` |
| `key_env` | `LITELLM_MASTER_KEY` | Name of the key in `secrets.env` sent as `Authorization: Bearer` |
| `chat_url` | | Where to open gateway answers. Placeholders: `{model}`, `{prompt}` |
| `auto_model` | `smart-router` | Gateway model shown as the **Auto** row |

The provider reads the gateway's `/model/info`, caching it for 60 s. If the gateway can't be reached,
it waits 30 s before trying again, and only app agents show in the meantime. The `model_info` fields it
uses are described in [PROVIDERS.md](PROVIDERS.md#model_info-fields).

### `[routing]`
| Key | Meaning |
|---|---|
| `low_effort_max_words` | Prompts this short get a low effort suggestion |
| `high_effort_patterns` | Regexes (case-insensitive) that suggest high effort |
| `[[routing.rule]]` | `tag`, `weight`, `patterns`: add `weight` to candidates with `tag` when any pattern matches |

Use TOML single-quoted strings for regexes (`'\bwhy\b'`), so backslashes don't need escaping.

## `secrets.env`
```
LITELLM_MASTER_KEY=sk-...
```
It holds only what the gateway needs. App agents use their own logins.
