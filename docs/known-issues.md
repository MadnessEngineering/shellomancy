# Known issues

Status as of 2026-10-31.

## Verified on the first launch (2026-10-31)

- `./script/run` builds `Shellomancy.app` (bundle id `cc.madnessinteractive.shellomancy`, URL scheme `shellomancy`, ad-hoc signed) and it launches.
- First launch creates `~/.mad/shellomancy` and `~/.mad/local-control` (directory `0700`, record and broker socket `0600`); local control is on by default.
- The Python client and the `shellomancy` CLI control the running app; `tab create --command` runs the command in the new tab.
- At about 3 and again at 11 minutes running: no outbound connections, only the loopback control listener, no Warp-server log lines ([privacy-and-offline.md](privacy-and-offline.md#results)).

## Not yet verified

- **The first-run screen** (no login, no pop-ups); the network checks, including the 10-minute idle one, passed ([privacy-and-offline.md](privacy-and-offline.md#results)).
- **The fork hook opening a real `/fork` as a Shellomancy tab**, and the MQTT bridge's `attach` against the real app (its state and `focus` were verified live).
- **Running beside stock Warp at the same time.**
- **The release bundle** (`./script/bundle --channel oss`).
- **The new brand assets in the app** (Dock icon variants, loading animation, About page logo at 350×100, the font glyph).

## Known gaps

- **Warp's local Agent Mode is still compiled in.** The Oz cloud features are off, but Warp's in-terminal agent, agent view and AI context menus remain. They're hidden while logged out, and Shellomancy can't log in. Turning off `AgentMode` affects about 50 code paths, and no Warp build ships without it, so it should be done with the app running to check each surface.
- **"Warp" still appears in the interface.** About 800 user-visible strings, menus ("Quit Warp"), links to `docs.warp.dev`, permission prompts outside the ones already changed, and the control CLI's help ("Control a running local Warp app instance").
- **No adaptive app icon.** The OSS channel has no `AppIcon.icon` bundle, so macOS icon tinting doesn't apply; the build prints a warning and uses the plain icon.
- **The Oz CLI wrapper** (`shellomancy-oz`) is still created by the release bundle script (`script/macos/bundle`), even though none of its commands work offline. Debug builds from `./script/run` don't create it.
- **SSH remote server** is off. Warp's version downloads Warp's CLI onto the remote host; Shellomancy has no replacement yet.
- **Signing.** Ad-hoc signed builds change signature on every rebuild, so macOS may ask for Keychain access again.
- **Only macOS on Apple Silicon** has been considered. Linux and Windows paths are updated in code and tests but never built.

## Failing tests

- **`cargo test -p warp_completer`** fails: 20 tests panic with "Tried to check FeatureFlag::CloudEnvironments before feature flags were initialized". It fails the same way before the `shellomancy` CLI rename. Whether it also fails on upstream `d57d9760` hasn't been checked.

## Distribution

- **Redistributing a built binary is an open question.** Five of Warp's git dependencies (`command-corrections`, `session-sharing-protocol`, `warp-command-signatures`, `warp-completion-metadata`, `warp-workflows`) declare no license yet (`about.toml`). Building from source for yourself is unaffected.
- **No releases, no installer, no auto-update.** Build from source ([README](../README.md#getting-it)).
