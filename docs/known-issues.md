# Known issues

Status as of 2026-10-31, at fork commit `e47e390b`.

## Not yet verified

- **Shellomancy has never been built into an app or launched.** Every change so far is checked by `cargo check`, unit and handler tests, and script syntax checks only. Next step: install Xcode and its Metal toolchain, then `./script/run` ([building.md](building.md)).
- **Offline behavior** is configured and unit-tested, but the running app hasn't been checked for outbound connections ([privacy-and-offline.md](privacy-and-offline.md#verifying-it)).
- **The `shellomancy` CLI and Python client** have only run against tests and a fake app, not against Shellomancy itself.
- **Running beside stock Warp**: separate paths, bundle id and Keychain items are set in code; nobody has run both at once yet.
- **The release bundle** (`./script/bundle --channel oss`) has not been run.
- **The wider About-page logo** ("shellomancy" wordmark) has not been seen at its 350×100 size.

## Known gaps

- **Warp's local Agent Mode is still compiled in.** The Oz cloud features are off, but Warp's in-terminal agent, agent view and AI context menus remain. They're hidden while logged out, and Shellomancy can't log in. Turning off `AgentMode` affects about 50 code paths, and no Warp build ships without it, so it waits until the app can be run and checked.
- **"Warp" still appears in the interface.** About 800 user-visible strings, menus ("Quit Warp"), links to `docs.warp.dev`, and permission prompts outside the ones already changed.
- **The Oz CLI wrapper** (`shellomancy-oz`) is still created in the app bundle, even though none of its commands work offline.
- **SSH remote server** is off. Warp's version downloads Warp's CLI onto the remote host; Shellomancy has no replacement yet.
- **Signing.** Ad-hoc signed builds change signature on every rebuild, so macOS may ask for Keychain access again.
- **Only macOS on Apple Silicon** has been considered. Linux and Windows paths are updated in code and tests but never built.

## Failing tests

- **`cargo test -p warp_completer`** fails: 20 tests panic with "Tried to check FeatureFlag::CloudEnvironments before feature flags were initialized". It fails the same way before the `shellomancy` CLI rename. Whether it also fails on upstream `d57d9760` hasn't been checked.

## Distribution

- **Redistributing a built binary is an open question.** Five of Warp's git dependencies (`command-corrections`, `session-sharing-protocol`, `warp-command-signatures`, `warp-completion-metadata`, `warp-workflows`) declare no license yet (`about.toml`). Building from source for yourself is unaffected.
- **No releases, no installer, no auto-update.** Build from source ([README](../README.md#getting-it)).
