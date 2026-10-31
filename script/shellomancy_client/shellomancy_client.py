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
    from shellomancy_client import open_tab
    open_tab(command="claude attach abc123", title="agent", color="cyan")

CLI:
    shellomancy_client.py tab-create --command "claude attach abc123" --title agent
    shellomancy_client.py instances
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


def open_tab(
    command: Optional[str] = None,
    cwd: Optional[str] = None,
    title: Optional[str] = None,
    color: Optional[str] = None,
    focus: bool = True,
    instance: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    """Open a terminal tab that runs `command` once its shell is ready.

    The app validates everything (no newlines/control characters in the
    command, `cwd` must be an existing absolute directory, `color` an ANSI color
    name) and requires the "Enabled, including running commands" Scripting mode
    for `command`.
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
    return send("tab.create", params, instance=instance)


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
    tab.add_argument("--instance")
    args = parser.parse_args(argv)
    try:
        if args.cmd == "instances":
            for record in list_instances():
                print(f"{record['instance_id']}\tpid {record['pid']}\t{record.get('started_at', '')}")
            return 0
        instance = select_instance(args.instance)
        data = open_tab(
            command=args.command,
            cwd=args.cwd,
            title=args.title,
            color=args.color,
            focus=not args.no_focus,
            instance=instance,
        )
        print(json.dumps(data))
        return 0
    except ShellomancyError as err:
        print(f"shellomancy: {err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(_main())
