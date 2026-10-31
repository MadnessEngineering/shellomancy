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

## Scripting modes

**Settings → Scripting → warpctrl CLI** has three modes. The value lives in the macOS Keychain, not in `settings.toml`, and only the app's Settings UI can change it.

| Mode | Effect |
|---|---|
| Disabled | No control API at all; the CLI reports `no_instance` |
| Enabled | Every action except running commands; `tab create --command` fails with `insufficient_permissions` |
| Enabled, including running commands | Everything. **Shellomancy's default.** |

Warp ships local control disabled. Shellomancy turns it on with commands so hooks work with no setup. The control API only accepts the same OS user (owner-only discovery directory and broker socket with a kernel peer-UID check, per-action short-lived tokens, loopback HTTP only), and a process running as you can already run commands. The new risk is that any such process can now also open visible terminal tabs. If you don't want that, choose **Enabled**.

## Other actions

The catalog is Warp's 84 actions, unchanged apart from `tab.create`'s new parameters. Groups: `instance`, `app`, `capability`, `window`, `tab`, `pane`, `session`, `input`, `theme`, `appearance`, `setting`, `keybinding`, `action`, `surface`, `file`. `shellomancy action list` prints them from the running app. Some surfaces open Warp features that are off or hidden in Shellomancy (Warp Drive, agent management); expect those to do nothing useful.

`input insert` and `input replace` only stage text in the input line; they never press Enter.

## From Python (hooks)

Each CLI call starts the whole app binary in control mode. Hooks should use the stdlib client instead: one socket round trip, Python 3.9+, no dependencies.

```python
import sys
sys.path.insert(0, "/path/to/shellomancy/script/shellomancy_client")
from shellomancy_client import open_tab, send, list_instances, ShellomancyError

open_tab(command="claude attach 1f8b2155", title="agent", color="cyan", focus=False)
send("theme.set", {"theme_name": "Dracula"})    # any catalog action
```

Or from a shell: `python3 script/shellomancy_client/shellomancy_client.py tab-create --command "…" --title agent`.

Errors raise `ShellomancyError` with the protocol's error code (`no_instance`, `insufficient_permissions`, `invalid_params`, …). With more than one running instance, the client uses the most recently started one unless you pass an `instance`.

The workshop's Claude Code fork hook uses this client to open background forks as Shellomancy tabs, falling back to a Warp window when Shellomancy isn't running.

## How it connects

1. **Discovery.** Each running app writes `<instance_id>.json` in `~/.mad/local-control` (or `$WARP_LOCAL_CONTROL_DISCOVERY_DIR`, or `$XDG_RUNTIME_DIR/mad/local-control`). The directory is `0700` and records are `0600`. A record names a loopback port and a broker socket. It never contains a credential.
2. **Credential.** The client connects to `<instance_id>.broker.sock` in the same directory and asks for a token for one action. The app checks the caller's user id and returns a bearer token valid for five minutes, for that action only.
3. **Request.** `POST http://127.0.0.1:<port>/v1/control` with `Authorization: Bearer <token>`, no `Origin` header, and a JSON envelope `{protocol_version, request_id, target, action: {kind, params}}`. The response is `{response: {status: "ok", data}}` or `{status: "error", error: {code, message}}`.

The wire details are in `specs/warp-control-cli/TECH.md`; [`shellomancy_client.py`](../script/shellomancy_client/shellomancy_client.py) is a short (under 300 lines) reference implementation.

A Finder-launched app doesn't inherit your shell's environment variables, so a discovery-directory override set in your shell won't reach it.
