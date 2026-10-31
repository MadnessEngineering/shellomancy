#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.9"
# dependencies = ["paho-mqtt>=2"]
# ///
"""Bridge a running Shellomancy app to the workshop's MQTT broker.

Publishes (same broker and device naming as the claude-prime hooks):

    status/<device>/shellomancy/state   retained; running instances and their tabs
    status/<device>/shellomancy/event   tab_opened / tab_closed / instance_started / instance_stopped
    status/<device>/shellomancy/reply   one reply per command: {request_id, ok, data | error}

Accepts commands (JSON payloads):

    command/<device>/shellomancy/attach  {"session_id": "1f8b2155...", "title"?, "cwd"?, "color"?, "focus"?, "request_id"?}
        opens a tab running `claude attach <id8>`
    command/<device>/shellomancy/focus   {"tab_id": "...", "instance_id"?, "request_id"?}

Never accepts a command string: `attach` builds `claude attach <id8>` itself
from a hex session id, so anyone who can publish to the broker can at most
attach to an existing Claude session or focus a tab.

Environment: MADNESS_MQTT_HOST (default localhost), MADNESS_MQTT_PORT (1883),
DeNa (device name, default macbook), SHELLOMANCY_MQTT_POLL (seconds, default 5).

Run: script/shellomancy_mqtt/shellomancy_mqtt.py   (uv fetches paho-mqtt)
"""

import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "shellomancy_client"))
import shellomancy_client  # noqa: E402

SESSION_ID_RE = re.compile(r"[0-9a-f]{8}(?:[0-9a-f-]{0,28})")
TAB_COLORS = {"red", "green", "yellow", "blue", "magenta", "cyan", "white", "black"}
MAX_TITLE = 80
MAX_REQUEST_ID = 64


class CommandError(Exception):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


def topics(device: str) -> Dict[str, str]:
    base = f"status/{device}/shellomancy"
    return {
        "state": f"{base}/state",
        "event": f"{base}/event",
        "reply": f"{base}/reply",
        "commands": f"command/{device}/shellomancy/+",
    }


def _clean_text(value: Any, field: str, limit: int) -> str:
    if not isinstance(value, str) or not value.strip():
        raise CommandError("invalid_params", f"{field} must be a non-empty string")
    if len(value) > limit or any(ch in value for ch in "\r\n") or any(ord(ch) < 32 for ch in value):
        raise CommandError("invalid_params", f"{field} is too long or has control characters")
    return value.strip()


class Bridge:
    """MQTT-agnostic core: turns commands into client calls and polls state."""

    def __init__(
        self,
        device: str,
        publish: Callable[[str, Dict[str, Any], bool], None],
        client=shellomancy_client,
        clock: Callable[[], float] = time.time,
    ):
        self.device = device
        self.topics = topics(device)
        self.publish = publish
        self.client = client
        self.clock = clock
        self._last_state: Optional[Dict[str, Any]] = None

    # -- commands -----------------------------------------------------------

    def handle_command(self, topic: str, raw: bytes) -> Dict[str, Any]:
        """Run one command and publish (and return) its reply."""
        request_id = None
        try:
            try:
                payload = json.loads(raw or b"{}")
            except ValueError:
                raise CommandError("invalid_request", "payload is not JSON")
            if not isinstance(payload, dict):
                raise CommandError("invalid_request", "payload must be a JSON object")
            if "request_id" in payload:
                request_id = _clean_text(payload["request_id"], "request_id", MAX_REQUEST_ID)
            kind = topic.rsplit("/", 1)[-1]
            if kind == "attach":
                data = self._attach(payload)
            elif kind == "focus":
                data = self._focus(payload)
            else:
                raise CommandError("unsupported_action", f"unknown command {kind!r}")
            reply = {"request_id": request_id, "command": kind, "ok": True, "data": data}
        except CommandError as err:
            reply = {"request_id": request_id, "ok": False, "error": {"code": err.code, "message": err.message}}
        except shellomancy_client.ShellomancyError as err:
            reply = {"request_id": request_id, "ok": False, "error": {"code": err.code, "message": err.message}}
        self.publish(self.topics["reply"], reply, False)
        return reply

    def _attach(self, payload: Dict[str, Any]) -> Dict[str, Any]:
        session_id = payload.get("session_id")
        if not isinstance(session_id, str) or not SESSION_ID_RE.fullmatch(session_id):
            raise CommandError("invalid_params", "session_id must be a lowercase hex Claude session id")
        id8 = session_id[:8]
        title = _clean_text(payload["title"], "title", MAX_TITLE) if "title" in payload else f"⑂ {id8}"
        color = payload.get("color")
        if color is not None and color not in TAB_COLORS:
            raise CommandError("invalid_params", f"color must be one of {sorted(TAB_COLORS)}")
        cwd = payload.get("cwd")
        if cwd is not None and (not isinstance(cwd, str) or not os.path.isabs(cwd)):
            raise CommandError("invalid_params", "cwd must be an absolute path")
        focus = payload.get("focus", True)
        if not isinstance(focus, bool):
            raise CommandError("invalid_params", "focus must be true or false")
        return self.client.open_tab(
            command=f"claude attach {id8}", cwd=cwd, title=title, color=color, focus=focus
        )

    def _focus(self, payload: Dict[str, Any]) -> Dict[str, Any]:
        tab_id = _clean_text(payload.get("tab_id"), "tab_id", MAX_REQUEST_ID)
        instance = None
        if "instance_id" in payload:
            instance = self.client.select_instance(_clean_text(payload["instance_id"], "instance_id", MAX_REQUEST_ID))
        target = {"tab": {"type": "id", "id": tab_id}}
        data = self.client.send("tab.activate", {"mode": "target"}, target=target, instance=instance)
        self.client.send("app.focus", {}, instance=instance)
        return data

    # -- state --------------------------------------------------------------

    def snapshot(self) -> Dict[str, Any]:
        instances = []
        for record in self.client.list_instances():
            entry = {
                "instance_id": record["instance_id"],
                "pid": record["pid"],
                "app_version": record.get("app_version"),
                "started_at": record.get("started_at"),
                "tabs": [],
            }
            try:
                entry["tabs"] = self.client.send("tab.list", {}, instance=record).get("tabs", [])
            except shellomancy_client.ShellomancyError as err:
                entry["error"] = err.code
            instances.append(entry)
        return {"device": self.device, "instances": instances}

    def poll(self) -> Tuple[Dict[str, Any], List[Dict[str, Any]]]:
        """Publish state if it changed, plus events for what changed."""
        state = self.snapshot()
        events = diff_events(self._last_state, state)
        if state != self._last_state:
            self.publish(self.topics["state"], dict(state, updated_at=self.clock()), True)
            for event in events:
                self.publish(self.topics["event"], dict(event, at=self.clock()), False)
            self._last_state = state
        return state, events


def diff_events(old: Optional[Dict[str, Any]], new: Dict[str, Any]) -> List[Dict[str, Any]]:
    def index(state):
        return {
            inst["instance_id"]: {tab["tab_id"]: tab for tab in inst.get("tabs", [])}
            for inst in (state or {}).get("instances", [])
        }

    if old is None:
        return []
    before, after = index(old), index(new)
    events: List[Dict[str, Any]] = []
    for instance_id in after.keys() - before.keys():
        events.append({"event": "instance_started", "instance_id": instance_id})
    for instance_id in before.keys() - after.keys():
        events.append({"event": "instance_stopped", "instance_id": instance_id})
    for instance_id in after.keys() & before.keys():
        for tab_id in after[instance_id].keys() - before[instance_id].keys():
            events.append({"event": "tab_opened", "instance_id": instance_id, "tab": after[instance_id][tab_id]})
        for tab_id in before[instance_id].keys() - after[instance_id].keys():
            events.append({"event": "tab_closed", "instance_id": instance_id, "tab_id": tab_id})
    return events


def main() -> int:
    import paho.mqtt.client as mqtt

    host = os.environ.get("MADNESS_MQTT_HOST", "localhost")
    port = int(os.environ.get("MADNESS_MQTT_PORT", "1883"))
    device = os.environ.get("DeNa", "macbook")
    interval = float(os.environ.get("SHELLOMANCY_MQTT_POLL", "5"))

    mq = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, client_id=f"shellomancy-bridge-{device}")

    def publish(topic, payload, retain):
        mq.publish(topic, json.dumps(payload), qos=1, retain=retain)

    bridge = Bridge(device, publish)

    def on_connect(client, userdata, flags, reason_code, properties):
        client.subscribe(bridge.topics["commands"], qos=1)
        print(f"shellomancy-mqtt: connected to {host}:{port} as {device}", file=sys.stderr)

    def on_message(client, userdata, message):
        reply = bridge.handle_command(message.topic, message.payload)
        status = "ok" if reply["ok"] else reply["error"]["code"]
        print(f"shellomancy-mqtt: {message.topic} -> {status}", file=sys.stderr)

    mq.on_connect = on_connect
    mq.on_message = on_message
    mq.will_set(bridge.topics["state"], json.dumps({"device": device, "instances": [], "bridge": "offline"}), qos=1, retain=True)
    # connect_async + loop_start keeps retrying, so a broker that is down at
    # startup (or restarts later) doesn't kill the bridge.
    mq.reconnect_delay_set(min_delay=1, max_delay=60)
    mq.connect_async(host, port, keepalive=30)
    mq.loop_start()
    try:
        while True:
            try:
                bridge.poll()
            except Exception as err:  # keep the bridge alive across app restarts
                print(f"shellomancy-mqtt: poll failed: {err}", file=sys.stderr)
            time.sleep(interval)
    except KeyboardInterrupt:
        return 0
    finally:
        mq.loop_stop()
        mq.disconnect()


if __name__ == "__main__":
    sys.exit(main())
