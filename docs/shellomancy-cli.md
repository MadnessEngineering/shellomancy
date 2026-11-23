# The `shellomancy` control CLI

`shellomancy` controls a running Shellomancy app from scripts: list and focus windows, open and rename tabs, split panes, switch themes, open surfaces, and (Shellomancy's addition) open a tab that runs a command. It is Warp's local-control CLI (`warpctrl`) under Shellomancy's name.

> **Verified live on 2026-10-31:** against a running Shellomancy, the Python client found the instance and `tab-create --command` opened a titled, colored tab whose shell ran the command; the `shellomancy` wrapper's `--help` (shows `shellomancy`), `tab list` and `tab create --no-focus` (kept the active tab) worked. Local control was on by default on first launch. Other actions haven't been tried.

The full upstream design is in [`specs/warp-control-cli/`](../specs/warp-control-cli/): `PRODUCT.md` (behavior and the action catalog), `SECURITY.md` (the threat model), `TECH.md` (implementation). Each starts with a note on where Shellomancy differs.

## Getting the command

The app bundle carries a wrapper at `Shellomancy.app/Contents/Resources/bin/shellomancy`. Put it on your `PATH` with **Settings → Scripting → Install** (symlinks `/usr/local/bin/shellomancy`, asking for an admin password), or call the wrapper directly. The wrapper runs the app binary in control mode; it doesn't open a window.

`shellomancy --help` and `shellomancy <group> --help` list everything. Help text uses the name you invoked, so an alias such as `madsh` reads naturally.

## Opening a tab that runs a command

```sh
shellomancy tab create --command "claude attach 1f8b2155" \
  --cwd ~/lab/madness_interactive --title "agent" --color cyan
```

| Flag | Meaning |
|---|---|
| `--command CMD` | Run `CMD` once the new tab's shell has started. It goes through the same pending-command path as tab configs and launch configs; nothing is typed into the terminal. |
| `--cwd DIR` | Start in `DIR`; relative paths resolve against your current directory. Without it, the tab starts where a normal new tab would. |
| `--title TEXT` | Tab title |
| `--color NAME` | Tab color: an ANSI color name (`red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, …) |
| `--no-focus` | Open the tab in the background. By default the app comes to the front and the new tab is active. |
| `--type terminal` | The only tab type these flags accept |

The app rejects the request, without creating a tab, if the command is empty, longer than 4096 bytes, or contains a newline or control character; if `--cwd` isn't an existing directory; if the title is blank; or if the color name is unknown. The response includes the new tab's id and `"command_queued": true`. "Queued" means the command is waiting for the shell, not that it has finished.

`shellomancy window create` does not accept these flags.

## Moving a tab to another window

```sh
shellomancy tab move-to-window --quake                         # the active tab, into the quake window
shellomancy tab move-to-window --window-index 0 --tab-index 2 --to-window-index 1
shellomancy tab move-to-window --new-window
```

Exactly one destination: `--quake` (opens the quake window around the tab when it isn't open), `--new-window`, `--to-window ID`, or `--to-window-index N` (`window list` order). The usual `--window…` / `--tab…` selectors pick the tab; by default it's the active tab of the active window. The tab moves whole, so whatever runs in its panes keeps running; moving a window's last tab closes that window. The response's `window_id` is where the tab landed. The app refuses (`invalid_params`) a move into the tab's own window, a new window for a window's only tab, and any move while a tab is being dragged.

This is `tab.move_to_window`, Shellomancy's addition to the catalog; the app's tab right-click menu (**Move to window**) and **⌥F3** (active tab to the quake window and back) do the same thing.

> Not yet run against a live app: covered by unit tests (the workspace move, parameter validation, the CLI's argument mapping).

## Scripting modes

**Settings → Scripting → warpctrl CLI** has three modes. The value lives in the macOS Keychain, not in `settings.toml`, and only the app's Settings UI can change it.

| Mode | Effect |
|---|---|
| Disabled | No control API at all; the CLI reports `no_instance` |
| Enabled | Every action except running commands; `tab create --command` fails with `insufficient_permissions` |
| Enabled, including running commands | Everything. **Shellomancy's default.** |

Warp ships local control disabled. Shellomancy turns it on with commands so hooks work with no setup. The control API only accepts the same OS user (owner-only discovery directory and broker socket with a kernel peer-UID check, per-action short-lived tokens, loopback HTTP only), and a process running as you can already run commands. The new risk is that any such process can now also open visible terminal tabs. If you don't want that, choose **Enabled**.

## Other actions

The catalog is Warp's 84 actions plus Shellomancy's `theme.window.set`, `theme.window.clear` and `tab.move_to_window`; of Warp's, only `tab.create` changed (its new parameters). Groups: `instance`, `app`, `capability`, `window`, `tab`, `pane`, `session`, `input`, `theme`, `appearance`, `setting`, `keybinding`, `action`, `surface`, `file`. `shellomancy action list` prints them from the running app. Some surfaces open Warp features that are off or hidden in Shellomancy (Warp Drive, agent management); expect those to do nothing useful.

`input insert` and `input replace` only stage text in the input line; they never press Enter.

## From Python (hooks)

Each CLI call starts the whole app binary in control mode. Hooks should use the stdlib client instead: one socket round trip, Python 3.9+, no dependencies.

```python
import sys
sys.path.insert(0, "/path/to/shellomancy/script/shellomancy_client")
from shellomancy_client import QUAKE, move_tab_to_window, open_tab, send, ShellomancyError

open_tab(command="claude attach 1f8b2155", title="agent", color="cyan", focus=False)
open_tab(command="claude --resume 1f8b2155", window_index=1)   # into a named window
move_tab_to_window(QUAKE)                                       # the active tab, into quake
send("theme.set", {"theme_name": "Dracula"})    # any catalog action
```

Or from a shell: `python3 script/shellomancy_client/shellomancy_client.py tab-create --command "…" --title agent`, or `… tab-move-to-window --quake`.

Without a window selector, actions use the active window. When Shellomancy is in the background no window is active and `tab-create` fails with `ambiguous_target`; pass `--window-index N` (or `window_index=N`) from `window.list`.

Errors raise `ShellomancyError` with the protocol's error code (`no_instance`, `insufficient_permissions`, `invalid_params`, …). With more than one running instance, the client uses the most recently started one unless you pass an `instance`.

The workshop's Claude Code fork hook uses this client to open background forks as Shellomancy tabs, falling back to a Warp window when Shellomancy isn't running.

## How it connects

1. **Discovery.** Each running app writes `<instance_id>.json` in `~/.mad/local-control` (or `$WARP_LOCAL_CONTROL_DISCOVERY_DIR`, or `$XDG_RUNTIME_DIR/mad/local-control`). The directory is `0700` and records are `0600`. A record names a loopback port and a broker socket. It never contains a credential.
2. **Credential.** The client connects to `<instance_id>.broker.sock` in the same directory and asks for a token for one action. The app checks the caller's user id and returns a bearer token valid for five minutes, for that action only.
3. **Request.** `POST http://127.0.0.1:<port>/v1/control` with `Authorization: Bearer <token>`, no `Origin` header, and a JSON envelope `{protocol_version, request_id, target, action: {kind, params}}`. The response is `{response: {status: "ok", data}}` or `{status: "error", error: {code, message}}`.

The wire details are in `specs/warp-control-cli/TECH.md`; [`shellomancy_client.py`](../script/shellomancy_client/shellomancy_client.py) is a short (about 400 lines) reference implementation.

A Finder-launched app doesn't inherit your shell's environment variables, so a discovery-directory override set in your shell won't reach it.
