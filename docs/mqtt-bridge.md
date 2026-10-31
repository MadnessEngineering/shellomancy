# MQTT bridge

[`script/shellomancy_mqtt/shellomancy_mqtt.py`](../script/shellomancy_mqtt/shellomancy_mqtt.py) connects a running Shellomancy to the workshop's MQTT broker. It publishes what Shellomancy has open and accepts a small, fixed set of commands. It is a separate process that talks to the app through the [control API](shellomancy-cli.md), so the app itself has no MQTT code.

> **Tested against a real broker and a fake app, not the real app.** An end-to-end test runs mosquitto, the bridge process and a stand-in Shellomancy; Shellomancy itself hasn't been launched yet ([known-issues.md](known-issues.md)).

## Running it

```sh
script/shellomancy_mqtt/shellomancy_mqtt.py
```

It's a `uv` script, so `uv` fetches `paho-mqtt` on first run. It uses the same settings as the claude-prime hooks:

| Variable | Default | Meaning |
|---|---|---|
| `MADNESS_MQTT_HOST` | `localhost` | broker host |
| `MADNESS_MQTT_PORT` | `1883` | broker port |
| `DeNa` | `macbook` | device name in topics |
| `SHELLOMANCY_MQTT_POLL` | `5` | seconds between state polls |

If the broker is down, the bridge keeps retrying. If Shellomancy isn't running, the state just lists no instances.

## Topics

| Topic | Direction | Payload |
|---|---|---|
| `status/<device>/shellomancy/state` | out, retained | `{device, updated_at, instances: [{instance_id, pid, app_version, started_at, tabs: [{tab_id, window_id, window_index, index, is_active}]}]}`. Published only when something changed. Set to `{"bridge": "offline"}` if the bridge drops off the broker. |
| `status/<device>/shellomancy/event` | out | `{event, instance_id, tab or tab_id, at}` where `event` is `tab_opened`, `tab_closed`, `instance_started` or `instance_stopped` |
| `command/<device>/shellomancy/attach` | in | `{session_id, title?, cwd?, color?, focus?, request_id?}` |
| `command/<device>/shellomancy/focus` | in | `{tab_id, instance_id?, request_id?}` |
| `status/<device>/shellomancy/reply` | out | `{request_id, command, ok, data}` or `{request_id, ok: false, error: {code, message}}` |

Example:

```sh
mosquitto_pub -t command/macbook/shellomancy/attach \
  -m '{"session_id": "1f8b2155-...", "color": "cyan", "request_id": "r1"}'
mosquitto_sub -t status/macbook/shellomancy/reply -C 1
```

## Commands

- **attach** opens a tab running `claude attach <first 8 characters of session_id>`. The bridge builds that command itself. `session_id` must be lowercase hex (a Claude session id or its 8-character prefix). Any other field naming a command is ignored. `title` defaults to `⑂ <id8>`. `color` is an ANSI color name. `cwd` must be absolute. `focus` defaults to true.
- **focus** activates a tab by id (from the state topic) and brings the app to the front.

Anything else gets an `unsupported_action` reply.

## Security

Anyone who can publish to the broker can use these commands. The bridge never accepts a command string, so the worst they can do is open a tab attached to an existing Claude session on this machine, or switch tabs. Shellomancy still applies its own checks (the scripting mode, field validation). If your broker is reachable beyond machines you trust, put authentication on it or don't run the bridge.

## Not built yet

- Tab titles and working directories in the state. `tab.list` doesn't return them; `tab.inspect` per tab could.
- Event push. The control API has no event stream, so changes are found by polling.
- A launchd agent to run the bridge at login.
