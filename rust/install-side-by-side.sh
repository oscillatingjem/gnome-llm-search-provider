#!/usr/bin/env bash
# Install the Rust build next to the Python one, under its own IDs ("Agent Search (Rust)"),
# so both can be compared live. Uses the same config (~/.llm-search/config).
#
#   rust/install-side-by-side.sh [--shortcut BINDING]     build and install
#   rust/install-side-by-side.sh --uninstall               remove it again
set -euo pipefail

HERE="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
LLM_SEARCH_HOME="${LLM_SEARCH_HOME:-$HOME/.llm-search}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
ENGINE="$DATA_HOME/llm-search-rs/engine"
RENDER="$LLM_SEARCH_HOME/state/install-rs"
XDG_DIR="$LLM_SEARCH_HOME/state/xdg"   # already on XDG_DATA_DIRS via the Python install's environment.d
APP="io.github.oscillatingjem.AgentSearchRs"
PROVIDER="$APP.SearchProvider"
NAME="Agent Search (Rust)"
SHORTCUT_PATH="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/llm-search-rs/"

say() { printf '%s\n' "$*"; }

links() {
  printf '%s\t%s\n' \
    "$XDG_DIR/gnome-shell/search-providers/$APP.search-provider.ini" "$RENDER/$APP.search-provider.ini" \
    "$DATA_HOME/dbus-1/services/$PROVIDER.service" "$RENDER/$PROVIDER.service" \
    "$DATA_HOME/dbus-1/services/$APP.service" "$RENDER/$APP.service" \
    "$DATA_HOME/applications/$APP.desktop" "$RENDER/$APP.desktop" \
    "$DATA_HOME/icons/hicolor/scalable/apps/$APP.svg" "$RENDER/$APP.svg" \
    "$HOME/.local/bin/llm-search-rs" "$ENGINE/llm-search"
}

stop_running() {  # by bus name, never by process name (which would match this script)
  local name pid
  for name in "$PROVIDER" "$APP"; do
    pid="$(gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
      --method org.freedesktop.DBus.GetConnectionUnixProcessID "$name" 2>/dev/null | sed -n 's/.*uint32 \([0-9]*\).*/\1/p' || true)"
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null && say "  stopped $name"
  done
  return 0
}

reload_bus() {
  gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus \
    --method org.freedesktop.DBus.ReloadConfig >/dev/null 2>&1 || true
}

set_shortcut() {  # $1 = binding or "" to remove
  local schema="org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:$SHORTCUT_PATH" list
  list="$(gsettings get org.gnome.settings-daemon.plugins.media-keys custom-keybindings)"
  if [[ -z "$1" ]]; then
    [[ "$list" == *"$SHORTCUT_PATH"* ]] || return 0
    list="$(python3 -I -c 'import ast,sys; l=ast.literal_eval(sys.argv[1].removeprefix("@as ")); print([x for x in l if x != sys.argv[2]])' "$list" "$SHORTCUT_PATH")"
    gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "$list"
    gsettings reset-recursively "$schema"
    say "  removed the shortcut"
    return 0
  fi
  gsettings set "$schema" name "$NAME"
  gsettings set "$schema" command "$HOME/.local/bin/llm-search-rs prompt"
  gsettings set "$schema" binding "$1"
  if [[ "$list" != *"$SHORTCUT_PATH"* ]]; then
    if [[ "$list" == "@as []" || "$list" == "[]" ]]; then list="['$SHORTCUT_PATH']"; else list="${list%]}, '$SHORTCUT_PATH']"; fi
    gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "$list"
  fi
  say "  shortcut $1 opens the Rust prompt box"
}

if [[ "${1:-}" == "--uninstall" ]]; then
  stop_running
  while IFS=$'\t' read -r target _; do
    [[ -L "$target" ]] && rm "$target" && say "  removed $target"
  done < <(links)
  rm -rf "$RENDER" "$DATA_HOME/llm-search-rs"
  set_shortcut ""
  reload_bus
  say "Removed the side-by-side Rust build. The Python install is untouched."
  exit 0
fi

shortcut=""
[[ "${1:-}" == "--shortcut" ]] && shortcut="${2:?--shortcut needs a binding}"

say "Building (release)…"
( cd "$HERE" && cargo build --release --locked 2>/dev/null || cargo build --release )
for bin in llm-search agent-search; do
  "$HERE/target/release/$bin" --help >/dev/null 2>&1 || [[ "$bin" == agent-search ]] || { echo "error: $bin doesn't run"; exit 1; }
done

say "Installing side by side as \"$NAME\""
mkdir -p "$ENGINE" "$RENDER"
install -m 755 "$HERE/target/release/llm-search" "$ENGINE/llm-search.new" && mv "$ENGINE/llm-search.new" "$ENGINE/llm-search"
install -m 755 "$HERE/target/release/agent-search" "$ENGINE/agent-search.new" && mv "$ENGINE/agent-search.new" "$ENGINE/agent-search"

cat > "$RENDER/$APP.search-provider.ini" <<EOF
[Shell Search Provider]
DesktopId=$APP.desktop
BusName=$PROVIDER
ObjectPath=/${PROVIDER//./\/}
Version=2
EOF
cat > "$RENDER/$PROVIDER.service" <<EOF
[D-BUS Service]
Name=$PROVIDER
Exec=/usr/bin/env AGENT_SEARCH_VARIANT=rs LLM_SEARCH_HOME=$LLM_SEARCH_HOME $ENGINE/llm-search search-provider
EOF
cat > "$RENDER/$APP.service" <<EOF
[D-BUS Service]
Name=$APP
Exec=/usr/bin/env AGENT_SEARCH_VARIANT=rs LLM_SEARCH_HOME=$LLM_SEARCH_HOME $ENGINE/agent-search --gapplication-service
EOF
cat > "$RENDER/$APP.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=$NAME
Comment=Send a prompt to Claude, local models and other AI agents (Rust build)
Icon=$APP
Exec=$HOME/.local/bin/llm-search-rs prompt
DBusActivatable=true
Categories=Utility;
Keywords=AI;LLM;Agent;Chat;Prompt;Claude;Spotlight;Launcher;Rust;
StartupNotify=true
EOF
cp "$REPO/share/io.github.oscillatingjem.AgentSearch.svg" "$RENDER/$APP.svg"

while IFS=$'\t' read -r target source; do
  mkdir -p "$(dirname "$target")"
  ln -sfn "$source" "$target"
done < <(links)

[[ -n "$shortcut" ]] && set_shortcut "$shortcut"
stop_running
reload_bus
command -v update-desktop-database >/dev/null && update-desktop-database -q "$DATA_HOME/applications" || true
command -v gtk4-update-icon-cache >/dev/null && gtk4-update-icon-cache -qtf "$DATA_HOME/icons/hicolor" 2>/dev/null || true

say ""
say "Done. Try it:"
say "  llm-search-rs query fix this python traceback"
say "  llm-search-rs prompt                      (opens the Rust prompt box)"
say "  Log out and back in once for \"$NAME\" to appear in GNOME search."
say "Remove it with: rust/install-side-by-side.sh --uninstall"
