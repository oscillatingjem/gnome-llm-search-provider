# Troubleshooting

## The Agent Search category doesn't appear
1. **Log out and back in.** GNOME Shell only looks for new providers at login.
   Afterwards, `systemctl --user show-environment | grep XDG_DATA_DIRS` should start with
   `~/.llm-search/state/xdg`. If it doesn't, check `~/.config/environment.d/60-llm-search.conf`.
   A provider `.ini` in `~/.local/share/gnome-shell/search-providers/` is **ignored**: GNOME Shell
   only reads that folder under the system data dirs.
2. **Settings → Search:** check that Agent Search is turned on.
3. `llm-search status`: every line should say `ok`.
4. Check that the query has at least `min_chars` characters (default 3), not counting `@`/`!` tokens.

## Check the provider directly
Ask the provider the way GNOME Shell does:
```bash
llm-search query explain btrfs snapshots
```
You should get a list of ids like `(['app:claude-chat||', 'app:claude-code||'],)`.
- `ServiceUnknown` error: the D-Bus service file isn't found. Re-run `./install.sh`, then
  `gdbus call --session --dest org.freedesktop.DBus --object-path /org/freedesktop/DBus --method org.freedesktop.DBus.ReloadConfig`.
- An empty list: check `agents.toml` for errors, and check `min_chars`.

## Logs
```bash
journalctl --user -b | grep -i -E 'llm-search|LlmSearchProvider'
```
To see errors as they happen, stop the provider and run it in a terminal:
```bash
llm-search status        # shows the provider's and prompt box's state
~/.local/share/llm-search/engine/llm-search search-provider   # run it in the foreground; errors go to the terminal
```

## Config changes don't apply
Both the provider and the prompt box reload `agents.toml` when it changes, on the next search or the next time the box opens. If something seems stale, run `./install.sh` again: it restarts both.

## Gateway models are missing
- Check `curl -H "Authorization: Bearer $KEY" http://127.0.0.1:4000/model/info`.
- After a failed request the provider waits 30 s before trying again, and it caches a successful list for 60 s.
- Check that `[gateway] url` is set and that `key_env` names a key that's in `secrets.env`.

## Pressing Enter does nothing
- App rows: check that the app handles its scheme (`xdg-mime query default x-scheme-handler/claude`).
- Gateway rows: `[gateway] chat_url` has to be set.

## The prompt box doesn't open, or opens without focus
- `llm-search status` should show the shortcut, and `llm-search prompt` should open the box.
- If the shortcut does nothing, check Settings → Keyboard → Custom Shortcuts for a conflict.
- If the box opens behind other windows or without keyboard focus, GNOME's focus-stealing prevention
  is in the way: please open an issue with your GNOME version.
- Errors: `journalctl --user -b | grep -i AgentSearch`, or run it in a terminal:
  `~/.local/share/llm-search/engine/agent-search` (after closing the running one: `./install.sh` restarts it).
