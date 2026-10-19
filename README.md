# Agent Search (gnome-llm-search-provider)

Send prompts to LLMs and AI agents straight from the GNOME search bar, or from a spotlight-style
prompt box on a keyboard shortcut.

Press <kbd>Super</kbd>, type a prompt, and pick where it goes. Your normal search results stay as they
are: this adds one more category, **Agent Search**, next to the others, like Firefox's "Search the web".
Or press your shortcut (for example <kbd>Ctrl</kbd>+<kbd>Super</kbd>+<kbd>Space</kbd>) to open the
**prompt box**: the same rows in a small window of its own, without the rest of GNOME's search.

```
Agent Search
  Claude Code (terminal)   “fix this python traceback” · low effort · coding
  Claude Code              “fix this python traceback” · coding
  Ask Claude               “fix this python traceback”
  Auto                     “fix this python traceback” · medium effort     ← with a gateway
  Qwen Coder (LAN)         “fix this python traceback” · coding, private   ← with a gateway
```

- **App agents.** Opens desktop apps through their URL links with the prompt filled in, or runs a
  command. Claude Desktop (chat and Claude Code) and Claude Code in a terminal (which runs the prompt
  immediately, with the model and effort you pick) are set up out of the box.
- **Prompt box.** A spotlight-style window with the same rows, opened from the app grid, a keyboard
  shortcut, or the category icon in search. It stays running after first use so it opens instantly.
- **Settings window.** The gear on the prompt box (or <kbd>Ctrl</kbd>+<kbd>,</kbd>) opens settings,
  which edit **everything** in the config:
  - **Agents:** every field, plus add, delete and rename;
  - **Routing:** aliases, effort patterns and ranking rules;
  - **General:** the keyboard shortcut and limits;
  - **Gateway:** the optional LiteLLM gateway.

  It writes to the same commented `agents.toml` you can edit by hand, keeping the comments. Changes
  apply immediately.
- **Any API model, through a gateway.** Point it at a [LiteLLM](https://github.com/BerriAI/litellm) proxy
  and every model the proxy serves shows up: OpenAI, Anthropic API, LM Studio, Ollama, vLLM, and others.
  Answers open in [Open WebUI](https://github.com/open-webui/open-webui) or any chat UI that takes the
  model and prompt in its URL.
- **Suggestions.** Rows are ranked for each prompt (for example, coding prompts put coding agents first),
  and each row shows why.
- **Choose a model and effort as you type.** `@opus !high refactor this` sends the prompt to that model at
  that effort level. See [docs/USAGE.md](docs/USAGE.md).

It uses GNOME's official search provider interface (D-Bus `SearchProvider2`), the same way Files and
Firefox add results. It doesn't patch GNOME Shell, so it isn't an extension and doesn't break when GNOME
Shell updates. It's written in Rust with GTK 4 and libadwaita. The search provider answers a cold
query in about 30 ms, and uses about 4 MB of memory.

## Requirements
- GNOME 46 or newer (GTK 4.14+, libadwaita 1.5+), running in a systemd user session; developed on GNOME 50
- To build: Rust 1.92 or newer, a C compiler, `pkg-config`, and the GTK 4, libadwaita, GLib and
  libsoup 3 development files
- Optional: `wl-clipboard` (for agents set to copy the prompt), and a LiteLLM gateway for API models

## Install

### 1. Install the build dependencies

| Distro | Command | Status |
|---|---|---|
| Fedora 43+ | `sudo dnf install rust cargo gcc pkgconf-pkg-config gtk4-devel libadwaita-devel glib2-devel libsoup3-devel wl-clipboard` | tested (Fedora 44) |
| Fedora Silverblue / Kinoite | Build in a toolbox (`toolbox create && toolbox enter`, then the Fedora line); the installed binaries run on the host | untested |
| Debian 13 / Ubuntu 24.04+ | `sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libglib2.0-dev libsoup-3.0-dev wl-clipboard`, plus Rust 1.92+ from [rustup.rs](https://rustup.rs) (Debian 13: or `trixie-backports`; Ubuntu 26.04's own `rustc` is new enough) | untested |
| Arch / Manjaro / EndeavourOS | `sudo pacman -S --needed base-devel rust gtk4 libadwaita libsoup3 wl-clipboard` | untested |
| openSUSE Tumbleweed | `sudo zypper install rust cargo gcc pkgconf-pkg-config gtk4-devel libadwaita-devel glib2-devel libsoup-devel wl-clipboard` | untested |

If you try a distro marked untested, please open an issue or PR to confirm it.

### 2. Clone and install
```bash
git clone https://github.com/oscillatingjem/gnome-llm-search-provider.git
cd gnome-llm-search-provider
./install.sh
```

The installer:
1. checks the build requirements and prints the install command for your distro if something is
   missing;
2. builds the two binaries (`cargo build --release`) and checks they run, **before** changing anything
   that's installed. It then copies them into `~/.local/share/llm-search/engine/`:
   - `llm-search`: the search provider and CLI;
   - `agent-search`: the prompt box and settings window;
3. creates the config location, `~/.llm-search/` (`config/` and `state/`), and copies the example
   configs without overwriting existing ones;
4. links the D-Bus services, desktop entry and icon into `~/.local/share`, and the `llm-search` command
   into `~/.local/bin`;
5. writes `~/.config/environment.d/60-llm-search.conf`, which adds `~/.llm-search/state/xdg` to your
   session's `XDG_DATA_DIRS`. GNOME Shell only reads search providers from system data folders, never
   from `~/.local/share`, so this lets it find the provider without root. The installer checks the
   result with systemd's own generator, and removes the file again if the check fails.

It's safe to run again. To update, run `git pull && ./install.sh`. Options:

| Option | Effect |
|---|---|
| `--dry-run` | Show what it would do without changing anything |
| `--name NAME` | Name shown in search and the app grid (default `Agent Search`) |
| `--shortcut BINDING` | Keyboard shortcut for the prompt box, e.g. `'<Primary><Super>space'` (Ctrl+Super+Space). Added as a GNOME custom shortcut, so you can change it in Settings → Keyboard |

To keep config somewhere else, set `LLM_SEARCH_HOME` when you install. The installer writes the path
into the D-Bus service file, so the provider uses it even though GNOME doesn't pass along your shell's environment:
```bash
LLM_SEARCH_HOME=~/.config/llm-search ./install.sh
```

### 3. Log out and back in
GNOME Shell only looks for new search providers at login, and the `XDG_DATA_DIRS` change also takes
effect only then. Afterwards, open **Settings → Search**
to check that **Agent Search** is turned on, and drag it to where you want it in the list.

**Upgrading from 0.5 (Python):** run `./install.sh`. It builds the Rust binaries and points the same
IDs at them, keeping your config and shortcut. No log-out is needed.

**Upgrading from 0.2 or earlier:** the app ID changed to `io.github.oscillatingjem.AgentSearch`. The
installer removes the old files, and you need to log out and back in once for the renamed category.

## Configure
Config lives in `~/.llm-search/config/`:

| File | What it holds |
|---|---|
| `agents.toml` | Everything: app agents, `@aliases`, ranking rules, limits, the optional gateway. The settings window edits this same file, keeping its comments |
| `secrets.env` | API keys for the gateway (mode 600; never committed) |

Changes apply on the next search or the next time the prompt box opens; no restart needed. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) and
[docs/PROVIDERS.md](docs/PROVIDERS.md).

## Commands
```
llm-search status          what's installed, and whether the provider is running
llm-search prompt [TEXT]   open the prompt box (with TEXT filled in)
llm-search query WORDS...  show the rows GNOME search would get
llm-search check           load the config and compile every routing pattern
llm-search uninstall       remove the binaries and links (add --purge to also delete ~/.llm-search)
```

## Develop
```bash
cargo test --workspace     # includes 577 cases recorded from the original Python implementation
./install.sh               # rebuild and reinstall
```
The code is a Cargo workspace:
- `crates/core`: logic, no GTK;
- `crates/platform`: GLib integration;
- `crates/cli`: the search provider and CLI;
- `crates/ui`: the prompt box and settings window.

See [docs/DESIGN.md](docs/DESIGN.md).

## Uninstall
```bash
llm-search uninstall           # keeps ~/.llm-search
llm-search uninstall --purge   # also deletes it
```
Then log out and back in.

## Documentation
- [docs/USAGE.md](docs/USAGE.md): search syntax, ranking, effort levels
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md): every config option
- [docs/PROVIDERS.md](docs/PROVIDERS.md): connecting Claude, LM Studio, Ollama, OpenAI and others
- [docs/DESIGN.md](docs/DESIGN.md): how it works
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)

## Roadmap
- `llm-search refresh`: generate the LiteLLM config from a short `providers.toml`, with automatic model
  discovery for LM Studio and Ollama, and effort variants
- Podman quadlets and a `compose.yaml` for running LiteLLM and Open WebUI
- Smarter ranking with a small local embedding model

## License
GPL-3.0-or-later. See [LICENSE](LICENSE).
