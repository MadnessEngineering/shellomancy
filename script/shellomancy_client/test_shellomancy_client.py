"""Tests for shellomancy_client against a fake app (real socket + HTTP server).

Run: python3 -m unittest discover -s script/shellomancy_client -v
"""

import json
import os
import socket
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from unittest import mock

import shellomancy_client as client

TOKEN = "test-token-not-a-secret"


class FakeApp:
    """A broker socket and an HTTP endpoint that check what the client sends."""

    def __init__(self, directory: Path, instance_id: str = "inst_test", reply=None):
        self.directory = directory
        self.instance_id = instance_id
        self.broker_requests = []
        self.http_requests = []
        self.reply = reply or (lambda envelope: {"status": "ok", "data": {"created": True}})

        self.broker = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.broker.bind(str(directory / f"{instance_id}.broker.sock"))
        self.broker.listen(4)
        threading.Thread(target=self._serve_broker, daemon=True).start()

        app = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):  # noqa: N802 (http.server API)
                length = int(self.headers["Content-Length"])
                envelope = json.loads(self.rfile.read(length))
                app.http_requests.append((self.path, dict(self.headers), envelope))
                response = app.reply(envelope)
                body = json.dumps(
                    {
                        "protocol_version": 1,
                        "request_id": envelope["request_id"],
                        "response": response,
                    }
                ).encode()
                self.send_response(200 if response.get("status") == "ok" else 400)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        self.http = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=self.http.serve_forever, daemon=True).start()

    def _serve_broker(self):
        while True:
            try:
                conn, _ = self.broker.accept()
            except OSError:
                return
            with conn:
                data = b""
                while True:
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                request = json.loads(data)
                self.broker_requests.append(request)
                conn.sendall(
                    json.dumps(
                        {
                            "bearer_token": TOKEN,
                            "grant": {
                                "credential_id": "cred_1",
                                "instance_id": self.instance_id,
                                "action": request["action"],
                                "issued_at": "2026-10-31T00:00:00Z",
                                "expires_at": "2026-10-31T00:05:00Z",
                            },
                        }
                    ).encode()
                )

    def write_record(self, **overrides):
        record = {
            "protocol_version": 1,
            "instance_id": self.instance_id,
            "pid": os.getpid(),
            "channel": "shellomancy",
            "app_id": "cc.madnessinteractive.shellomancy",
            "app_version": None,
            "started_at": "2026-10-31T00:00:00Z",
            "executable_path": None,
            "endpoint": {"host": "127.0.0.1", "port": self.http.server_address[1]},
            "credential_broker": {"socket_path": f"{self.instance_id}.broker.sock"},
            "actions": [],
        }
        record.update(overrides)
        (self.directory / f"{self.instance_id}.json").write_text(json.dumps(record))
        return record

    def close(self):
        self.http.shutdown()
        self.http.server_close()
        self.broker.close()


class ClientTest(unittest.TestCase):
    def setUp(self):
        # Short path: macOS limits Unix socket paths to 104 bytes.
        self.tmp = tempfile.TemporaryDirectory(prefix="smc")
        self.dir = Path(self.tmp.name)
        self.env = mock.patch.dict(os.environ, {client.DISCOVERY_DIR_ENV: str(self.dir)})
        self.env.start()
        self.app = FakeApp(self.dir)

    def tearDown(self):
        self.app.close()
        self.env.stop()
        self.tmp.cleanup()

    def test_open_tab_runs_broker_then_http_flow(self):
        self.app.write_record()
        data = client.open_tab(command="claude attach abc123", title="agent", color="cyan", focus=False)

        self.assertEqual(data, {"created": True})
        self.assertEqual(len(self.app.broker_requests), 1)
        self.assertEqual(self.app.broker_requests[0]["action"], "tab.create")
        self.assertEqual(self.app.broker_requests[0]["protocol_version"], 1)

        path, headers, envelope = self.app.http_requests[0]
        self.assertEqual(path, "/v1/control")
        self.assertEqual(headers["Authorization"], f"Bearer {TOKEN}")
        self.assertNotIn("Origin", headers)
        self.assertEqual(headers["Host"], f"127.0.0.1:{self.app.http.server_address[1]}")
        self.assertEqual(envelope["protocol_version"], 1)
        self.assertEqual(envelope["target"], {})
        self.assertEqual(
            envelope["action"],
            {
                "kind": "tab.create",
                "params": {
                    "command": "claude attach abc123",
                    "title": "agent",
                    "color": "cyan",
                    "focus": False,
                },
            },
        )

    def test_open_tab_sends_absolute_cwd_and_omits_focus_by_default(self):
        self.app.write_record()
        client.open_tab(cwd="~")
        params = self.app.http_requests[0][2]["action"]["params"]
        self.assertEqual(params, {"cwd": os.path.expanduser("~")})

    def test_app_error_is_raised_with_its_code(self):
        self.app.reply = lambda envelope: {
            "status": "error",
            "error": {"code": "insufficient_permissions", "message": "running commands is disabled"},
        }
        self.app.write_record()
        with self.assertRaises(client.ShellomancyError) as raised:
            client.open_tab(command="true")
        self.assertEqual(raised.exception.code, "insufficient_permissions")

    def test_list_instances_skips_unusable_records(self):
        self.app.write_record()
        good = client.list_instances()
        self.assertEqual([r["instance_id"] for r in good], ["inst_test"])

        for overrides in [
            {"channel": "stable"},
            {"protocol_version": 2},
            {"pid": 2**22 + 12345},  # not a live process
            {"endpoint": {"host": "0.0.0.0", "port": 1}},
            {"credential_broker": {"socket_path": "../elsewhere.sock"}},
            {"endpoint": None, "credential_broker": None},  # local control disabled
        ]:
            self.app.write_record(**overrides)
            self.assertEqual(client.list_instances(), [], overrides)

    def test_no_instance_raises(self):
        with self.assertRaises(client.ShellomancyError) as raised:
            client.open_tab(command="true")
        self.assertEqual(raised.exception.code, "no_instance")

    def test_discovery_dir_defaults_to_mad(self):
        with mock.patch.dict(os.environ, {"HOME": "/home/x"}, clear=True):
            self.assertEqual(client.discovery_dir(), Path("/home/x/.mad/local-control"))
        with mock.patch.dict(os.environ, {"XDG_RUNTIME_DIR": "/run/u"}, clear=True):
            self.assertEqual(client.discovery_dir(), Path("/run/u/mad/local-control"))


if __name__ == "__main__":
    unittest.main()
