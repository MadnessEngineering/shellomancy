#!/usr/bin/env python3
"""Talk to a running Shellomancy app over its local-control API (stdlib only).

Hooks and scripts should use this instead of the `shellomancy` control CLI:
the CLI starts the whole app binary for every call, this is a socket round
trip.

Protocol (crates/local_control; see specs/warp-control-cli/TECH.md):

1. Discovery: each running app writes `<instance_id>.json` into the private
   discovery directory (`$WARP_LOCAL_CONTROL_DISCOVERY_DIR`, else
   `$XDG_RUNTIME_DIR/mad/local-control`, else `~/.mad/local-control`).
2. Credential: connect to `<instance_id>.broker.sock` in the same directory,
   send `{"protocol_version", "request_id", "action"}`, half-close, read a
   short-lived bearer token scoped to that one action.
3. Request: POST the request envelope to `http://127.0.0.1:<port>/v1/control`
   with `Authorization: Bearer <token>` and no Origin header.

Tokens stay in memory; never log or print them.

Example:
    from shellomancy_client import QUAKE, move_tab_to_window, open_tab
    open_tab(command="claude attach abc123", title="agent", color="cyan")
    move_tab_to_window(QUAKE)

CLI:
    shellomancy_client.py tab-create --command "claude attach abc123" --title agent
    shellomancy_client.py tab-create --window-index 1 --command "claude --resume <id>"
    shellomancy_client.py tab-move-to-window --quake
    shellomancy_client.py tab-move-to-window --window-index 0 --tab-index 2 --to-window-index 1
    shellomancy_client.py instances

Window and tab selectors default to the active window and its active tab. When
no Shellomancy window is active (the app is in the background), name the
window with `--window-index` (`window.list` order) or `--window` (its id).
"""

import argparse
import http.client
import json
import os
import socket
import sys
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional

PROTOCOL_VERSION = 1
CHANNEL = "shellomancy"
DISCOVERY_DIR_ENV = "WARP_LOCAL_CONTROL_DISCOVERY_DIR"
BROKER_SOCKET_SUFFIX = ".broker.sock"
TIMEOUT_SECONDS = 10.0


class ShellomancyError(Exception):
    """A local-control error; `code` matches the protocol's ErrorCode names."""

    def __init__(self, code: str, message: str, details: Optional[str] = None):
        super().__init__(f"{code}: {message}" + (f" ({details})" if details else ""))
        self.code = code
        self.message = message
        self.details = details


def discovery_dir() -> Path:
    override = os.environ.get(DISCOVERY_DIR_ENV)
    if override:
        return Path(override)
    runtime = os.environ.get("XDG_RUNTIME_DIR")
    if runtime:
        return Path(runtime) / "mad" / "local-control"
    return Path(os.environ.get("HOME", ".")) / ".mad" / "local-control"


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def _authority_ok(record: Dict[str, Any]) -> bool:
    """Mirror InstanceRecord::validate_local_control_authority."""
    endpoint = record.get("endpoint")
    broker = record.get("credential_broker")
    if endpoint is None or broker is None:
        return False  # local control disabled for this instance
    return endpoint.get("host") == "127.0.0.1" and broker.get(
        "socket_path"
    ) == f"{record['instance_id']}{BROKER_SOCKET_SUFFIX}"


def list_instances(channel: str = CHANNEL) -> List[Dict[str, Any]]:
    """Live, control-enabled instances for `channel`, newest first."""
    directory = discovery_dir()
    records = []
    try:
        paths = sorted(directory.glob("*.json"))
    except OSError:
        return []
    for path in paths:
        try:
            record = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        if not isinstance(record, dict):
            continue
        if record.get("protocol_version") != PROTOCOL_VERSION:
            continue
        if record.get("channel") != channel:
            continue
        if not isinstance(record.get("pid"), int) or not _pid_alive(record["pid"]):
            continue
        if not _authority_ok(record):
            continue
        records.append(record)
    records.sort(key=lambda r: r.get("started_at", ""), reverse=True)
    return records


def select_instance(instance_id: Optional[str] = None, channel: str = CHANNEL) -> Dict[str, Any]:
    instances = list_instances(channel)
    if instance_id is not None:
        instances = [r for r in instances if r["instance_id"] == instance_id]
    if not instances:
        raise ShellomancyError(
            "no_instance",
            f"no running {channel} app with local control enabled in {discovery_dir()}",
        )
    return instances[0]


def _raise_error_payload(payload: Any, fallback: str) -> None:
    error = payload.get("error") if isinstance(payload, dict) else None
    if isinstance(error, dict) and "code" in error:
        raise ShellomancyError(error["code"], error.get("message", ""), error.get("details"))
    raise ShellomancyError("transport_unavailable", fallback)


def request_credential(instance: Dict[str, Any], action: str) -> str:
    """Ask the instance's broker for a bearer token scoped to `action`."""
    path = discovery_dir() / f"{instance['instance_id']}{BROKER_SOCKET_SUFFIX}"
    request = {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": str(uuid.uuid4()),
        "action": action,
    }
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(TIMEOUT_SECONDS)
            sock.connect(str(path))
            sock.sendall(json.dumps(request).encode())
            sock.shutdown(socket.SHUT_WR)
            chunks = []
            while True:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                chunks.append(chunk)
    except OSError as err:
        raise ShellomancyError(
            "transport_unavailable", "failed to reach the credential broker", str(err)
        ) from err
    try:
        payload = json.loads(b"".join(chunks))
    except ValueError as err:
        raise ShellomancyError(
            "transport_unavailable", "credential broker returned invalid JSON"
        ) from err
    token = payload.get("bearer_token") if isinstance(payload, dict) else None
    if isinstance(token, str) and token:
        return token
    _raise_error_payload(payload, "credential broker returned no token")
    raise AssertionError("unreachable")


def send(
    action: str,
    params: Optional[Dict[str, Any]] = None,
    target: Optional[Dict[str, Any]] = None,
    instance: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """Send one action and return the response `data`, or raise ShellomancyError."""
    instance = instance or select_instance()
    token = request_credential(instance, action)
    envelope = {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": str(uuid.uuid4()),
        "target": target or {},
        "action": {"kind": action, "params": params or {}},
    }
    endpoint = instance["endpoint"]
    connection = http.client.HTTPConnection("127.0.0.1", endpoint["port"], timeout=TIMEOUT_SECONDS)
    try:
        # http.client sends Host: 127.0.0.1:<port> and no Origin header, which
        # is exactly what the app's loopback checks require.
        connection.request(
            "POST",
            "/v1/control",
            body=json.dumps(envelope),
            headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
        )
        response = connection.getresponse()
        body = response.read()
    except OSError as err:
        raise ShellomancyError(
            "transport_unavailable", "failed to send local-control request", str(err)
        ) from err
    finally:
        connection.close()
    try:
        payload = json.loads(body)
    except ValueError as err:
        raise ShellomancyError(
            "transport_unavailable", f"local-control request failed with HTTP {response.status}"
        ) from err
    inner = payload.get("response") if isinstance(payload, dict) else None
    if isinstance(inner, dict):
        if inner.get("status") == "ok":
            return inner.get("data") or {}
        _raise_error_payload(inner, f"local-control request failed with HTTP {response.status}")
    _raise_error_payload(payload, f"local-control request failed with HTTP {response.status}")
    raise AssertionError("unreachable")


QUAKE = {"type": "quake"}
"""`move_tab_to_window` destination: the quake window, opened around the tab when it isn't open."""

NEW_WINDOW = {"type": "new_window"}
"""`move_tab_to_window` destination: a new window holding just the tab."""


def window_selector(
    window_id: Optional[str] = None, window_index: Optional[int] = None
) -> Optional[Dict[str, Any]]:
    """A window selector by opaque id or by `window.list` index; None for the active window."""
    if window_id is not None and window_index is not None:
        raise ValueError("pass window_id or window_index, not both")
    if window_id is not None:
        return {"type": "id", "id": window_id}
    if window_index is not None:
        return {"type": "index", "index": window_index}
    return None


def window_destination(
    window_id: Optional[str] = None, window_index: Optional[int] = None
) -> Dict[str, Any]:
    """`move_tab_to_window` destination: an open window, by opaque id or `window.list` index."""
    selector = window_selector(window_id, window_index)
    if selector is None:
        raise ValueError("a window destination needs window_id or window_index")
    return {"type": "window", "window": selector}


def _target(
    window_id: Optional[str] = None,
    window_index: Optional[int] = None,
    tab_index: Optional[int] = None,
) -> Dict[str, Any]:
    target: Dict[str, Any] = {}
    window = window_selector(window_id, window_index)
    if window is not None:
        target["window"] = window
    if tab_index is not None:
        target["tab"] = {"type": "index", "index": tab_index}
    return target


def open_tab(
    command: Optional[str] = None,
    cwd: Optional[str] = None,
    title: Optional[str] = None,
    color: Optional[str] = None,
    focus: bool = True,
    window_id: Optional[str] = None,
    window_index: Optional[int] = None,
    instance: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """Open a terminal tab that runs `command` once its shell is ready.

    The app validates everything (no newlines/control characters in the
    command, `cwd` must be an existing absolute directory, `color` a tab color:
    an ANSI name, bright_<name>, a fixed hue such as "ember", or "#rrggbb") and
    requires the "Enabled, including running commands" Scripting mode for
    `command`.
    """
    params: Dict[str, Any] = {}
    if command is not None:
        params["command"] = command
    if cwd is not None:
        params["cwd"] = os.path.abspath(os.path.expanduser(cwd))
    if title is not None:
        params["title"] = title
    if color is not None:
        params["color"] = color
    if not focus:
        params["focus"] = False
    return send(
        "tab.create",
        params,
        target=_target(window_id, window_index),
        instance=instance,
    )


def move_tab_to_window(
    destination: Dict[str, Any],
    tab_index: Optional[int] = None,
    window_id: Optional[str] = None,
    window_index: Optional[int] = None,
    instance: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """Move a tab into another window; whatever runs in it keeps running.

    `destination` is QUAKE, NEW_WINDOW, or `window_destination(...)`. The tab is
    the active tab of the active window unless `tab_index` and a window are
    given. Returns the acknowledgement, whose `window_id` is where it landed.
    """
    return send(
        "tab.move_to_window",
        {"destination": destination},
        target=_target(window_id, window_index, tab_index),
        instance=instance,
    )


def _add_window_arguments(parser: argparse.ArgumentParser) -> None:
    window = parser.add_mutually_exclusive_group()
    window.add_argument("--window", help="window id from window.list (default: the active window)")
    window.add_argument("--window-index", type=int, help="window index from window.list")
    parser.add_argument("--instance")


def _main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Shellomancy local-control client")
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("instances", help="list running Shellomancy instances")
    tab = sub.add_parser("tab-create", help="open a tab, optionally running a command")
    tab.add_argument("--command")
    tab.add_argument("--cwd")
    tab.add_argument("--title")
    tab.add_argument("--color")
    tab.add_argument("--no-focus", action="store_true")
    _add_window_arguments(tab)
    move = sub.add_parser(
        "tab-move-to-window", help="move a tab into another window; it keeps running"
    )
    move.add_argument("--tab-index", type=int, help="tab index in its window (default: active tab)")
    _add_window_arguments(move)
    destination = move.add_mutually_exclusive_group(required=True)
    destination.add_argument("--quake", action="store_true", help="the quake window")
    destination.add_argument("--new-window", action="store_true", help="a new window of its own")
    destination.add_argument("--to-window", help="the window with this id")
    destination.add_argument("--to-window-index", type=int, help="the window at this index")
    args = parser.parse_args(argv)
    try:
        if args.cmd == "instances":
            for record in list_instances():
                print(f"{record['instance_id']}\tpid {record['pid']}\t{record.get('started_at', '')}")
            return 0
        instance = select_instance(args.instance)
        if args.cmd == "tab-move-to-window":
            if args.quake:
                to = QUAKE
            elif args.new_window:
                to = NEW_WINDOW
            else:
                to = window_destination(args.to_window, args.to_window_index)
            data = move_tab_to_window(
                to,
                tab_index=args.tab_index,
                window_id=args.window,
                window_index=args.window_index,
                instance=instance,
            )
        else:
            data = open_tab(
                command=args.command,
                cwd=args.cwd,
                title=args.title,
                color=args.color,
                focus=not args.no_focus,
                window_id=args.window,
                window_index=args.window_index,
                instance=instance,
            )
        print(json.dumps(data))
        return 0
    except ShellomancyError as err:
        print(f"shellomancy: {err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(_main())
