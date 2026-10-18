# Connecting providers

There are two kinds of target:

| Kind | Configured in | Examples |
|---|---|---|
| **App agents**: desktop apps opened through a link | `agents.toml` `[[app]]` | Claude Desktop, Claude Code |
| **Gateway models**: API models behind one LiteLLM proxy | the LiteLLM config, plus `[gateway]` in `agents.toml` | LM Studio, Ollama, vLLM, OpenAI, Anthropic API |

## Claude Desktop and Claude Code (subscription)
Set up out of the box in `examples/config/agents.toml`:

```toml
[[app]]
id = "claude-chat"
name = "Ask Claude"
url = "claude://claude.ai/new?q={prompt}"

[[app]]
id = "claude-code"
name = "Claude Code"
url = "claude://code/new?q={prompt}"
```

- These links work with a Claude subscription. A subscription can't be used through LiteLLM; for that you
  need an Anthropic API key (see below).
- `claude://code/new` also accepts `&folder=/absolute/path` (more than once is fine) to start the
  session in a project.
- The links take no model or effort. Claude uses whatever is set in the app.
- **They only fill in the prompt; you press Enter.** The app has no link option to send it automatically.

### Claude Code in a terminal (runs immediately)
With the [Claude Code CLI](https://claude.ai/install.sh) installed and logged in, this row starts the
session with the prompt already running, using the model and effort you chose:
```toml
[[app]]
id = "claude-cli"
name = "Claude Code (terminal)"
command = ["ptyxis", "--new-window", "--", "claude",
           ["--model", "{model}"], ["--effort", "{effort}"], "--", "{prompt}"]
cwd = "~"
requires = ["ptyxis", "claude"]
models = ["opus", "sonnet", "haiku", "fable"]
efforts = ["low", "medium", "high", "xhigh", "max"]
```
For other terminals, replace the start of `command`:
- GNOME Console: `["kgx", "--", …]`
- GNOME Terminal: `["gnome-terminal", "--", …]`
- Kitty: `["kitty", …]`

Set `cwd` to the project folder you usually work in.

Any other app with a URL scheme works the same way. Use `{model}` and `{effort}` in `url` if its links
accept them, and list them in `models` and `efforts`.

## A gateway for API models
[LiteLLM Proxy](https://docs.litellm.ai/) serves one OpenAI-compatible API for every provider. The
provider reads its model list (`/model/info`), so **models you add to LiteLLM show up in search
automatically**.

> **Pin the version.** LiteLLM releases 1.82.7 and 1.82.8 on PyPI were compromised in March 2026. Run a
> pinned, known-good container image tag and check release notes before upgrading.

### 1. Run LiteLLM and a chat UI
Bind both to localhost. Replace `<tag>` with a pinned release.
```bash
podman run -d --name litellm -p 127.0.0.1:4000:4000 \
  --env-file ~/.llm-search/config/secrets.env \
  -v ~/.llm-search/config/litellm.yaml:/app/config.yaml:ro,Z \
  ghcr.io/berriai/litellm:<tag> --config /app/config.yaml

podman run -d --name open-webui -p 127.0.0.1:3000:8080 \
  -e OPENAI_API_BASE_URL=http://host.containers.internal:4000/v1 \
  -e OPENAI_API_KEY="$LITELLM_MASTER_KEY" \
  -v open-webui:/app/backend/data ghcr.io/open-webui/open-webui:<tag>
```
Generated Podman quadlets (systemd services) are on the roadmap.

### 2. Point the provider at it
```toml
# agents.toml
[gateway]
url = "http://127.0.0.1:4000"
key_env = "LITELLM_MASTER_KEY"
chat_url = "http://127.0.0.1:3000/?models={model}&q={prompt}"   # Open WebUI
auto_model = "smart-router"
```
`chat_url` can be any chat UI that takes the model and prompt in its URL.

### 3. Add models to the LiteLLM config
Each entry's `model_info` controls how it appears in search.

```yaml
model_list:
  # LM Studio on another machine (LAN or Tailscale)
  - model_name: coder
    litellm_params:
      model: lm_studio/qwen3-coder-30b
      api_base: http://lmstudio-host:1234/v1
      api_key: os.environ/LMSTUDIO_API_KEY   # only if LM Studio's API token is on
    model_info: { display_name: "Qwen Coder (LAN)", tags: [coding, private], aliases: [qwen] }

  # Ollama
  - model_name: llama
    litellm_params: { model: ollama_chat/llama3.2, api_base: http://ollama-host:11434 }
    model_info: { display_name: "Llama (local)", tags: [private, fast] }

  # OpenAI, with effort variants: one entry per level, sharing a `base`
  - model_name: gpt@low
    litellm_params: { model: openai/gpt-5, reasoning_effort: low, api_key: os.environ/OPENAI_API_KEY }
    model_info: { base: gpt, effort: low, display_name: "GPT", tags: [reasoning, coding] }
  - model_name: gpt@high
    litellm_params: { model: openai/gpt-5, reasoning_effort: high, api_key: os.environ/OPENAI_API_KEY }
    model_info: { base: gpt, effort: high }

  # Anthropic API (API key, not a Claude subscription)
  - model_name: claude-api@high
    litellm_params: { model: anthropic/claude-opus-5-5, reasoning_effort: high, api_key: os.environ/ANTHROPIC_API_KEY }
    model_info: { base: claude-api, effort: high, display_name: "Claude API", tags: [reasoning, coding, writing] }

  # Auto: LiteLLM's Auto Router picks a tier, and so the model and effort
  - model_name: smart-router
    litellm_params:
      model: auto_router/complexity_router
      complexity_router_config:
        tiers: { SIMPLE: llama, MEDIUM: coder, COMPLEX: gpt@high, REASONING: gpt@high }
      complexity_router_default_model: llama
    model_info: { display_name: "Auto" }
```
Model names and router options change between LiteLLM releases, so check the
[LiteLLM docs](https://docs.litellm.ai/docs/auto_router) for your version.

### `model_info` fields
| Field | Meaning |
|---|---|
| `display_name` | Row title (default: the model name) |
| `tags` | Ranking tags, the same vocabulary as the `[[routing.rule]]` tags |
| `aliases` | Extra `@names` |
| `base`, `effort` | Group effort variants: entries with the same `base` show as one row, with `effort` as its levels |
| `priority` | Added to the ranking score |
| `icon` | Icon name |

### Remote and self-hosted servers
- **LAN:** use the server's LAN address in `api_base`.
- **Away from home:** use Tailscale or WireGuard, and point `api_base` at the tunnel address.
- **Don't expose LM Studio, Ollama or vLLM to the internet without authentication.** If you must, turn
  on the server's API key and keep it in `secrets.env`.
