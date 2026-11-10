# Known issues

Status as of 2026-10-31.

## Verified on the first launch (2026-10-31)

- `./script/run` builds `Shellomancy.app` (bundle id `cc.madnessinteractive.shellomancy`, URL scheme `shellomancy`, ad-hoc signed) and it launches.
- First launch creates `~/.mad/shellomancy` and `~/.mad/local-control` (directory `0700`, record and broker socket `0600`); local control is on by default.
- The Python client and the `shellomancy` CLI control the running app; `tab create --command` runs the command in the new tab.
- At about 3 and again at 11 minutes running: no outbound connections, only the loopback control listener, no Warp-server log lines ([privacy-and-offline.md](privacy-and-offline.md#results)).
- The first launch opens straight to a terminal: no login screen, no Warp pop-ups.

## Not yet verified

- **The fork hook opening a real `/fork` as a Shellomancy tab**, and the MQTT bridge's `attach` against the real app (its state and `focus` were verified live).
- **Running beside stock Warp at the same time.**
- **The release bundle** (`./script/bundle --channel oss`).
- **The new brand assets in the app** (Dock icon variants, loading animation, About page logo at 350×100, the font glyph).

## Known gaps

- **Agent Mode is off, but not every surface has been looked at.** With `AgentMode` and 17 related flags off, the app starts and the control API's agent surfaces decline cleanly. Upstream never ships this combination, so some UI may still offer agent features or render oddly; report anything you find. MCP settings and the harness flags are still on. The repo symbol outline (a tree-sitter parse of every file in each repo a pane enters, used only by Agent Mode's `@` menu) no longer runs with Agent Mode off. Before that, entering a 38,000-file repo kept two cores busy for about 6 minutes in a debug build; after the change (verified 2026-11-02), a tab in the same repo logged no outline parse and the app sat near 1% CPU. The separate repository metadata index (file list and watchers) still runs.
- **"Warp" still appears in the interface.** About 800 user-visible strings, menus ("Quit Warp"), links to `docs.warp.dev`, and permission prompts outside the ones already changed. The control CLI's own output, help, and errors, and its bundled agent skill (`shelloctrl`, formerly `warpctrl`), now say Shellomancy. The skill still points at the command palette's "Install Warp Control CLI command" entries by their current labels.
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
