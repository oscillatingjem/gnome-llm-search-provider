# Usage

Press <kbd>Super</kbd> and type. Once the query has at least 3 characters (`min_chars`), the **Agents**
category shows up to 5 rows (`max_rows`). Use the arrow keys to pick one and press <kbd>Enter</kbd>, or click
the category icon to send the prompt to the top row.

## Picking a model and effort
Tokens can go anywhere in the query; everything else is the prompt.

| Token | Meaning | Examples |
|---|---|---|
| `@name` | Send to this agent or model. Matches aliases, ids, names and model names, by prefix. | `@code`, `@claude`, `@coder`, `@son` |
| `!level` | Effort level: `low` (`l`), `medium` (`m`, `med`), `high` (`h`), `max` (`x`) | `!high`, `!l` |

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

App agents get effort only if their links accept it. Claude Desktop's don't, so Claude uses the effort
and model set in the app.
