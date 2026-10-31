"""Tests for the Shellomancy MQTT bridge core (no broker, no app).

Run: python3 -m unittest discover -s script/shellomancy_mqtt -v
"""

import importlib.util
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

import shellomancy_mqtt as bridge_mod
from shellomancy_client import ShellomancyError


class FakeClient:
    """Stands in for shellomancy_client; records calls."""

    def __init__(self):
        self.calls = []
        self.instances = [{"instance_id": "inst_a", "pid": 1, "started_at": "t0", "app_version": None}]
        self.tabs = {"inst_a": [{"tab_id": "t1", "index": 0, "is_active": True}]}
        self.fail_with = None

    def list_instances(self):
        return list(self.instances)

    def select_instance(self, instance_id=None):
        for record in self.instances:
            if instance_id in (None, record["instance_id"]):
                return record
        raise ShellomancyError("no_instance", "none")

    def open_tab(self, **kwargs):
        self.calls.append(("open_tab", kwargs))
        if self.fail_with:
            raise ShellomancyError(self.fail_with, "refused")
        return {"created": True, "command_queued": True}

    def send(self, action, params=None, target=None, instance=None):
        self.calls.append(("send", action, params, target, instance))
        if action == "tab.list":
            return {"tabs": self.tabs.get(instance["instance_id"], [])}
        return {"ok": True}


class BridgeTest(unittest.TestCase):
    def setUp(self):
        self.published = []
        self.client = FakeClient()
        self.bridge = bridge_mod.Bridge(
            "macbook",
            lambda topic, payload, retain: self.published.append((topic, payload, retain)),
            client=self.client,
            clock=lambda: 1000.0,
        )

    def command(self, name, payload):
        return self.bridge.handle_command(
            f"command/macbook/shellomancy/{name}", json.dumps(payload).encode()
        )

    def test_attach_builds_the_command_from_a_validated_id(self):
        reply = self.command("attach", {"session_id": "1f8b2155-aaaa-bbbb-cccc-000000000000", "request_id": "r1", "color": "cyan"})

        self.assertTrue(reply["ok"], reply)
        self.assertEqual(reply["request_id"], "r1")
        kind, kwargs = self.client.calls[0]
        self.assertEqual(kind, "open_tab")
        self.assertEqual(kwargs["command"], "claude attach 1f8b2155")
        self.assertEqual(kwargs["title"], "⑂ 1f8b2155")
        self.assertEqual(kwargs["color"], "cyan")
        self.assertTrue(kwargs["focus"])
        self.assertEqual(self.published, [("status/macbook/shellomancy/reply", reply, False)])

    def test_attach_rejects_anything_that_is_not_a_hex_session_id(self):
        for bad in ["1f8b2155; rm -rf ~", "$(whoami)", "1F8B2155", "abc", "", 12345678, None]:
            reply = self.command("attach", {"session_id": bad})
            self.assertFalse(reply["ok"], bad)
            self.assertEqual(reply["error"]["code"], "invalid_params")
        self.assertEqual(self.client.calls, [])

    def test_attach_never_takes_a_command_field(self):
        reply = self.command("attach", {"session_id": "1f8b2155", "command": "rm -rf ~"})
        self.assertTrue(reply["ok"])
        self.assertEqual(self.client.calls[0][1]["command"], "claude attach 1f8b2155")

    def test_attach_validates_optional_fields(self):
        for payload in [
            {"title": "two\nlines"},
            {"title": "x" * 200},
            {"color": "chartreuse"},
            {"cwd": "relative/dir"},
            {"focus": "yes"},
        ]:
            reply = self.command("attach", dict(payload, session_id="1f8b2155"))
            self.assertEqual(reply["error"]["code"], "invalid_params", payload)
        self.assertEqual(self.client.calls, [])

    def test_app_errors_are_relayed(self):
        self.client.fail_with = "insufficient_permissions"
        reply = self.command("attach", {"session_id": "1f8b2155"})
        self.assertEqual(reply["error"]["code"], "insufficient_permissions")

    def test_bad_payloads_and_unknown_commands(self):
        reply = self.bridge.handle_command("command/macbook/shellomancy/attach", b"not json")
        self.assertEqual(reply["error"]["code"], "invalid_request")
        reply = self.command("exec", {"command": "ls"})
        self.assertEqual(reply["error"]["code"], "unsupported_action")

    def test_focus_activates_the_tab_then_raises_the_app(self):
        reply = self.command("focus", {"tab_id": "t1"})
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(
            self.client.calls[0][1:4],
            ("tab.activate", {"mode": "target"}, {"tab": {"type": "id", "id": "t1"}}),
        )
        self.assertEqual(self.client.calls[1][1], "app.focus")

    def test_poll_publishes_retained_state_only_when_it_changes(self):
        self.bridge.poll()
        self.bridge.poll()
        states = [p for p in self.published if p[0] == "status/macbook/shellomancy/state"]
        self.assertEqual(len(states), 1)
        topic, payload, retain = states[0]
        self.assertTrue(retain)
        self.assertEqual(payload["instances"][0]["tabs"], [{"tab_id": "t1", "index": 0, "is_active": True}])
        self.assertEqual(payload["updated_at"], 1000.0)

    def test_poll_emits_tab_and_instance_events(self):
        self.bridge.poll()
        self.client.tabs["inst_a"] = [{"tab_id": "t2", "index": 0, "is_active": True}]
        self.client.instances.append({"instance_id": "inst_b", "pid": 2, "started_at": "t1", "app_version": None})
        _, events = self.bridge.poll()

        kinds = sorted((e["event"], e.get("tab_id") or e.get("tab", {}).get("tab_id") or e["instance_id"]) for e in events)
        self.assertEqual(
            kinds,
            [("instance_started", "inst_b"), ("tab_closed", "t1"), ("tab_opened", "t2")],
        )
        published_events = [p for p in self.published if p[0] == "status/macbook/shellomancy/event"]
        self.assertEqual(len(published_events), 3)
        self.assertTrue(all(not retain for _, _, retain in published_events))

    def test_first_poll_emits_no_events(self):
        _, events = self.bridge.poll()
        self.assertEqual(events, [])


HERE = Path(__file__).resolve().parent
MOSQUITTO = shutil.which("mosquitto") or "/opt/homebrew/sbin/mosquitto"


def _has_paho():
    return importlib.util.find_spec("paho") is not None


@unittest.skipUnless(
    os.path.exists(MOSQUITTO) and shutil.which("mosquitto_pub") and _has_paho(),
    "needs a mosquitto broker binary, mosquitto clients and paho-mqtt",
)
class EndToEndTest(unittest.TestCase):
    """Real broker + real bridge process + fake Shellomancy app."""

    def setUp(self):
        sys.path.insert(0, str(HERE.parent / "shellomancy_client"))
        spec = importlib.util.spec_from_file_location(
            "client_tests", HERE.parent / "shellomancy_client" / "test_shellomancy_client.py"
        )
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self.tmp = tempfile.TemporaryDirectory(prefix="smq")
        self.app = module.FakeApp(Path(self.tmp.name))
        self.app.write_record()

        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            self.port = probe.getsockname()[1]
        self.broker = subprocess.Popen(
            [MOSQUITTO, "-p", str(self.port)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )
        time.sleep(0.5)
        env = dict(
            os.environ,
            MADNESS_MQTT_HOST="127.0.0.1",
            MADNESS_MQTT_PORT=str(self.port),
            DeNa="e2e",
            SHELLOMANCY_MQTT_POLL="0.5",
            WARP_LOCAL_CONTROL_DISCOVERY_DIR=self.tmp.name,
        )
        self.bridge = subprocess.Popen(
            [sys.executable, str(HERE / "shellomancy_mqtt.py")],
            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        time.sleep(1.5)

    def tearDown(self):
        self.bridge.terminate()
        self.bridge.wait(5)
        self.broker.terminate()
        self.broker.wait(5)
        self.app.close()
        self.tmp.cleanup()

    def _sub(self, topic, count=1, timeout=5):
        return subprocess.Popen(
            ["mosquitto_sub", "-h", "127.0.0.1", "-p", str(self.port), "-t", topic,
             "-C", str(count), "-W", str(timeout)],
            stdout=subprocess.PIPE, text=True,
        )

    def test_attach_command_round_trip_and_retained_state(self):
        reply_sub = self._sub("status/e2e/shellomancy/reply")
        time.sleep(0.5)
        subprocess.run(
            ["mosquitto_pub", "-h", "127.0.0.1", "-p", str(self.port),
             "-t", "command/e2e/shellomancy/attach",
             "-m", json.dumps({"session_id": "1f8b2155", "request_id": "e2e-1"})],
            check=True,
        )
        reply = json.loads(reply_sub.communicate(timeout=10)[0])
        self.assertTrue(reply["ok"], reply)
        self.assertEqual(reply["request_id"], "e2e-1")
        sent = self.app.http_requests[-1][2]["action"]
        self.assertEqual(sent["kind"], "tab.create")
        self.assertEqual(sent["params"]["command"], "claude attach 1f8b2155")

        state = json.loads(self._sub("status/e2e/shellomancy/state").communicate(timeout=10)[0])
        self.assertEqual(state["device"], "e2e")
        self.assertEqual(state["instances"][0]["instance_id"], "inst_test")


if __name__ == "__main__":
    unittest.main()
