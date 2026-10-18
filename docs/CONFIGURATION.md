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

`LLM_SEARCH_HOME` is read when you run `install.sh`, and the installer writes it into the D-Bus service
file. To move your config, re-run the installer with the new value.

The provider reads config when it starts, and it exits after 60 s without searches. To apply a change
right away, run `pkill -f llm_search`; the next search starts it again.

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
| `url` | yes | Link to open. Placeholders: `{prompt}`, `{model}`, `{effort}`, all URL-encoded. Query parameters that end up empty are dropped. |
| `icon` | | Icon name from your icon theme |
| `tags` | | Capability tags for ranking (`coding`, `writing`, `reasoning`, `private`, `fast`, …) |
| `models` | | Models the link can choose (`{model}`); lets `@model` pick one |
| `efforts` | | Effort levels the link accepts (`{effort}`) |
| `priority` | | Added to the ranking score (default 0) |
| `copy_prompt` | | Also copy the prompt to the clipboard (for apps whose links can't carry it) |

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
