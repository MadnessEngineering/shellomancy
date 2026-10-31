# Privacy and offline mode

Shellomancy is built to never contact Warp's servers and never send telemetry or crash reports. Checks on the running app agree, including one after more than 10 minutes (see [Results](#results)); the first-run screen is still to be confirmed by eye.

## What is off, and how

| What | Upstream behavior (logged out) | Shellomancy | Where |
|---|---|---|---|
| Warp server, realtime and Oz URLs | `app.warp.dev`, `rtc.app.warp.dev`, `oz.warp.dev` | `127.0.0.1:9` (a closed loopback port, refused instantly); session-sharing URL unset | `WarpServerConfig::offline`, `OzConfig::offline` in `crates/warp_core/src/channel/config.rs` |
| Update check | `GET /client_version` every 10 minutes in release builds | Off (`Autoupdate` flag forced off) | `OFFLINE_DISABLED_FEATURES` |
| Changelog | Fetched at window creation in release builds | Off (`Changelog`) | same |
| First-run onboarding | Queries Warp's public model list | Off (`AgentOnboarding`, `AccountFirstOnboarding`); first launch goes straight to a terminal | same |
| Launch pop-ups for Warp products | Shown once | Off | same |
| SSH remote server | Downloads Warp's CLI from `app.warp.dev` onto remote hosts | Off (`SshRemoteServer`) | same |
| Crash reports | Sentry, when compiled in | Not compiled in, no destination, and `CrashReporting` forced off | same; `app/src/bin/oss.rs` |
| Telemetry | RudderStack | No destination, and the send function returns early | `app/src/server/telemetry/mod.rs` |
| Install detection | Loopback HTTP server that `*.warp.dev` web pages may query | Not started | `app/src/lib.rs` |
| Login, Warp AI, Warp Drive, session sharing | Need a Warp account | Unavailable; Settings hides their pages | `app/src/settings_view/mod.rs` |

The flags are forced off after every other source can turn them on (cargo features, release-bundle flags, debug flags), in `init_feature_flags`. Shellomancy's OSS channel also ignores server-URL overrides from flags or environment variables, so the offline config can't be redirected at runtime.

To undo the offline mode (for example, to log in to Warp), change `server_config` and `oz_config` in `app/src/bin/oss.rs` back to `production()`.

## What can still use the network

These are user actions, not Warp services:

- **Language servers**: enabling one may download it from GitHub, npm or nodejs.org.
- **Your shell, prompt and plugins**: whatever they do on their own.
- **Links you click**: they open in your browser.

## Local listeners

The one listener Shellomancy deliberately exposes is the control API, while scripting is enabled: `127.0.0.1` at a random port, plus an owner-only Unix socket in `~/.mad/local-control`. See [shellomancy-cli.md](shellomancy-cli.md#scripting-modes) to turn it off. Warp's processes also talk to each other locally (for example, the app and its terminal-server helper); those haven't been audited for Shellomancy yet.

## Verifying it

Planned check, not yet run:

1. Start Shellomancy, open a few tabs, and leave it idle for more than 10 minutes (longer than the upstream update poll).
2. `lsof -nP -a -p "$(pgrep -f Shellomancy.app/Contents/MacOS/warp-oss)" -i` should show only loopback sockets.
3. `~/Library/Logs/shellomancy.log` should not contain "Starting autoupdate polling loop", "Fetching channel versions", "Start to send telemetry events to RudderStack", or "Failed to fetch free-tier LLMs".
4. First run: delete `~/.mad/shellomancy` and relaunch; it should open straight to a terminal with no login screen or pop-ups.

For an extra guarantee you can point `app.warp.dev`, `rtc.app.warp.dev`, `sessions.app.warp.dev` and `oz.warp.dev` at `0.0.0.0` in `/etc/hosts`.

### Results

- **2026-10-31, first launch, about 3 minutes running** (fresh `~/.mad`, a few tabs opened through the control API): the app and its terminal-server helper had one inet socket, the control API listening on `127.0.0.1` (no install-detection listener, no outbound connections). The log had none of the lines above, and no mention of `warp.dev`, RudderStack or Sentry. The log showed the logged-out path ("Unable to read user from secure storage: NotFound"), with no errors.
- **2026-10-31, 11 minutes 23 seconds running** (past the upstream 10-minute update poll; three tabs, control-API and MQTT-bridge traffic during the first minutes): still one inet socket, the loopback control listener. No update, changelog, telemetry, model-list, `client_version`, `warp.dev`, RudderStack or Sentry lines in the log; no errors. Steps 1–3 pass.
- **Still to do:** step 4, a look at the first-run screen.
