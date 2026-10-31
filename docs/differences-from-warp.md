# How Shellomancy differs from Warp

Shellomancy forks [warpdotdev/warp](https://github.com/warpdotdev/warp) at `d57d9760` and builds Warp's `oss` channel. Each difference below names the commit that implements it and how it has been checked. "Tests" means `cargo test` / `cargo check` ran and passed for that commit; nothing here has been checked in a running app yet (see [known-issues.md](known-issues.md)).

Code changes carry a `Shellomancy:` comment, so `rg "Shellomancy:"` lists them.

## Identity

| Difference | Commit | Checked by |
|---|---|---|
| Bundle id `cc.madnessinteractive.shellomancy`, app name `Shellomancy.app`, URL scheme `shellomancy://`, log `~/Library/Logs/shellomancy.log` | `6bea79bd` | path tests, `cargo check` |
| Settings in `~/.mad/shellomancy`; the terminal-only (TUI) build in `~/.mad/shellomancy-cli`, never the GUI's directory | `6bea79bd` | path tests (incl. a TUI-separation test) |
| Does not use Warp's App Group container (`2BBY89MBSN.dev.warp`), so its database never lands in stock Warp's container | `6bea79bd` | unit test |
| Control-API discovery in `~/.mad/local-control` (Warp uses `~/.warp/local-control` for every channel) | `6bea79bd` | tests |
| SSH remote server installs to `~/.mad/shellomancy/remote-server` on remote hosts (upstream reused Warp Dev's `~/.warp-dev`) | `bc3f61d5` | remote_server tests |
| TUI app id `cc.madnessinteractive.shellomancy-tui` | `65831e03` | `cargo check` |
| The binary is still named `warp-oss`, to keep the diff against upstream small | — | — |

## Branding

| Difference | Commit | Checked by |
|---|---|---|
| Nautilus app icon (`app/channels/oss/icon/shellomancy.svg`) | `95818bd7` | rendered PNG reviewed |
| In-app Warp marks, 12-frame loading animation, login and About logos, debug-build Dock icon, and the logo glyph in the bundled Roboto fonts redrawn as the nautilus; file names stay `warp*` so code is unchanged | `556b2df0` | rendered review, `cargo check`, byte-identical regeneration |
| Dock icon picker offers Default, Abyss, Bone, Ember, Mono, Toxic instead of Warp's 16 icons | `556b2df0` | `cargo check` |
| About page: "A fork of Warp. Copyright 2026 Denver Technologies, Inc.", the AGPL-3.0 notice, and a link to this repository | `2c4600d6` | `cargo check` |

Regenerate the brand assets with [`script/shellomancy_brand`](../script/shellomancy_brand/README.md).

## Offline

| Difference | Commit | Checked by |
|---|---|---|
| Server and Oz URLs point at a closed loopback port (`127.0.0.1:9`), so nothing reaches Warp's servers; login, Warp AI, Drive and session sharing are unavailable | `65831e03` | unit tests |
| Forced off: auto-update, changelog, agent onboarding, account-first onboarding, Warp's launch pop-ups, Warp-hosted SSH remote server | `65831e03` | unit tests |
| Forced off: crash reporting and handoff checkpoints (turned on by release bundles) | `e47e390b` | unit tests |
| Telemetry is never sent (no destination, and an early return in the send path) | `65831e03` | `cargo check` |
| The loopback install-detection server that `*.warp.dev` pages query is not started | `6a4cb483` | `cargo check` |

Details: [privacy-and-offline.md](privacy-and-offline.md).

## Not an agent platform

| Difference | Commit | Checked by |
|---|---|---|
| 28 flags for Warp's Oz cloud-agent platform forced off: cloud mode and cloud conversations/environments/runners, ambient and scheduled agents, agent management, Oz platform skills/handoff/identity, local↔cloud handoff, agent shared sessions, Warp-hosted Factory MCP, remote codebase indexing, Drive objects as context | `881c2f4c` | unit test (also asserts the CLI-agent flags stay on) |
| Settings hides Billing, Referrals, Shared blocks, Teams, Warp Drive, Warp Agent, Agent profiles, Knowledge, Cloud environments, Cloud agent API keys | `064e2a09` | settings tests |
| No "Install Oz CLI" palette entries; the Oz CLI is renamed `shellomancy-oz` | `881c2f4c`, `ce9f2a54` | `cargo check` |
| Kept: support for Claude Code, Codex and other CLI agents running in the terminal | — | unit test |
| Not yet changed: Warp's local Agent Mode and agent view are still compiled in (hidden while logged out, and Shellomancy can't log in) | — | see known-issues |

## Scripting (local control)

| Difference | Commit | Checked by |
|---|---|---|
| The control CLI is named `shellomancy` (Warp: `warpctrl`); help shows the name you invoked, so aliases work | `ce9f2a54` | `warp_cli` tests |
| Local control is on by default, including a new mode that allows running commands; Settings → Scripting can lower it | `8127378c` | settings tests |
| `tab create` takes `--command`, `--cwd`, `--title`, `--color`, `--no-focus`; the command runs once the tab's shell is ready, through the same path tab configs use | `8127378c` | handler tests |
| A stdlib Python client for hooks | `7d73c120` | tests against a fake app |
| An MQTT bridge: state and tab events out, `attach` / `focus` commands in ([mqtt-bridge.md](mqtt-bridge.md)) | `script/shellomancy_mqtt` | unit tests + end-to-end test with a real mosquitto broker and a fake app |
| Release bundle script keeps your `--features` and ships the `shellomancy` wrapper | `152d83a9`, `ce9f2a54` | `bash -n` only |

Details: [shellomancy-cli.md](shellomancy-cli.md).

## Build scripts

| Difference | Commit | Checked by |
|---|---|---|
| `./script/run` always builds the `oss` channel, includes `warp_control_cli`, and doesn't run Warp's remote agent-skills installer unless asked | `6bea79bd` | `bash -n` |
| `./script/bundle --channel oss` uses the Shellomancy identity | `152d83a9` | `bash -n` |
