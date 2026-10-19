# Usage

Press <kbd>Super</kbd> and type. Once the query has at least 3 characters (`min_chars`), the **Agent Search**
category shows up to 5 rows (`max_rows`). Use the arrow keys to pick one and press <kbd>Enter</kbd>, or click
the category icon to send the prompt to the top row.

## The prompt box
A small window with one text field and the same rows as the search bar. Open it with:
- the keyboard shortcut chosen at install (`./install.sh --shortcut '<Primary><Super>space'`), which
  you can change in Settings → Keyboard → Custom Shortcuts;
- **Agent Search** in the app grid;
- the **Agent Search** icon next to its rows in GNOME search, which carries over what you typed;
- `llm-search prompt [text]` from a terminal or script.

<kbd>Enter</kbd> sends to the selected row (the top one by default), <kbd>↑</kbd>/<kbd>↓</kbd> or
<kbd>Tab</kbd> choose another, and <kbd>Esc</kbd> or clicking elsewhere closes it. After closing, it
keeps running for `[prompt] idle_minutes` (default 30) so it reopens instantly.

The **gear** next to the text field, or <kbd>Ctrl</kbd>+<kbd>,</kbd>, opens **settings**:
- **General:**
  - the keyboard shortcut (press the new combination, or Backspace to remove it);
  - how long the box keeps running, and result limits;
  - **Restore Defaults**;
  - links to `agents.toml` and these docs.
- **Agents:** every field of every agent. That covers name, id, link, command, working folder, required
  programs, tags, models, effort levels, icon, priority, copy-prompt and on/off. You can also add,
  delete and rename agents; renaming updates aliases that point at the agent.
- **Routing:** aliases, the patterns that suggest high effort, the low-effort word limit, and ranking
  rules (tag, weight, patterns), each with add, edit and delete. Patterns turn red while they aren't
  valid.
- **Gateway:** the LiteLLM URL, chat URL, Auto model, key name and key, with a **Test** button.

Every change is saved straight to `agents.toml`, keeping its comments, and only if the whole config is
still valid.

## Picking a model and effort
Tokens can go anywhere in the query; everything else is the prompt.

| Token | Meaning | Examples |
|---|---|---|
| `@name` | Send to this agent or model. Matches aliases, ids, names and model names, by prefix. | `@code`, `@claude`, `@coder`, `@son` |
| `!level` | Effort level: `low` (`l`), `medium` (`m`, `med`), `high` (`h`), `xhigh` (`xh`), `max` (`x`) | `!high`, `!l` |

```
@code why does this test hang          → Claude Code first
!low capital of Peru                   → every row that supports effort uses low
@coder !high explain this regex        → that gateway model, high-effort variant
```

The model you name with `@` always comes first. Only the first `@` and the first `!` token count; later
ones stay in the prompt, as do `@` and `!` inside words (`bob@example.com`, `wow!`).

## How rows are chosen
1. The model named with `@`, if any.
2. **Auto** (the gateway's router), if a gateway is set up.
3. The other agents and models, ranked: each `[[routing.rule]]` whose patterns match the prompt adds its
   weight to every candidate carrying that tag, on top of the candidate's `priority`.
4. The best app agent is always kept in the list, even when gateway models outrank it.

Each row's description shows the prompt, the effort (when the target supports one) and the tags that matched.

## Effort
With no `!level`, one is suggested: **high** if a `high_effort_patterns` entry matches (why, design,
debug, compare, …), **low** for prompts of `low_effort_max_words` words or fewer, **medium** otherwise
(**high** above 60 words). If a target doesn't support the level asked for, the closest one it does
support is used. Targets without effort levels show none.

App agents get a model and effort only if their link or command accepts them. Claude Desktop's links
don't, so those rows use the app's own settings. The **Claude Code (terminal)** row runs the
`claude` CLI with `--model` and `--effort`, so `@opus !high fix this` works there, and the prompt runs
immediately.
