# Building Shellomancy

macOS on Apple Silicon only. Shellomancy builds Warp's `oss` channel (binary `warp-oss`, bundled as `Shellomancy.app`).

> **Verified once.** `./script/run` built and launched Shellomancy on 2026-10-31 (macOS 27, Xcode 27.2, M-series, 10 cores). The release bundle path is still unverified.

## Prerequisites

| Need | Why | Check |
|---|---|---|
| Xcode (full app, not just Command Line Tools) | `crates/warpui/build.rs` compiles Metal shaders with `xcrun -sdk macosx metal` | `xcodebuild -version` |
| Xcode's Metal toolchain component | separate download on recent Xcode | `xcrun -sdk macosx metal --version` |
| Rust 1.92 | pinned in `rust-toolchain.toml`; rustup picks it inside the repo | `rustc --version` (run in the repo) |
| `cargo-bundle` 0.11 | `./script/run` bundles the `.app` with it | `cargo bundle --version` |
| `protoc`, `jq`, `git-lfs` | codegen (`crates/remote_server`), scripts, the few LFS assets | `which protoc jq git-lfs` |

One-time setup after installing Xcode:

```sh
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
xcodebuild -runFirstLaunch
xcodebuild -downloadComponent MetalToolchain
cargo +1.92.0 install cargo-bundle --version 0.11.0 --locked
```

`cargo-bundle` needs Rust 1.88+, so install it with the repo's toolchain (`+1.92.0`) if your default `rustc` is older.

You don't need Warp's `./script/bootstrap`. It installs Warp's own tooling (brew packages, PowerShell, sentry-cli, gcloud auth, Warp's shared agent skills) that Shellomancy doesn't use.

## Run a debug build

```sh
./script/run
```

This builds `target/debug/bundle/osx/Shellomancy.app` (ad-hoc signed unless you have an "Apple Development" identity), adds the `shellomancy` control-CLI wrapper at `Contents/Resources/bin/shellomancy`, and launches the app as a child of your shell. Useful flags:

- `--dont-open` — build without launching
- `--release` — optimized build (`target/release/bundle/osx/Shellomancy.app`)
- `--open_with_launchd` — launch through `open` and tail `~/Library/Logs/shellomancy.log`
- `--install-common-skills` — opt in to Warp's shared agent-skills installer (it downloads and runs a script from Warp's repository; off by default in Shellomancy)

Differences from upstream `script/run`: it always builds the `oss` channel (upstream switches to Warp's internal channel if a private config tool is on `PATH`), includes `warp_control_cli` by default, and skips the agent-skills installer.

Measured on the first run (2026-10-31): `./script/run --dont-open` took 332 s, 4 m 52 s of it in cargo, but most dependencies were already built by earlier `cargo check` runs, so a truly clean build will take longer. `target/` reached 36 GB. `cargo bundle` also writes a DMG to `target/debug/bundle/dmg/`. The only warning was the missing adaptive `AppIcon.icon` (see known-issues).

## Install it (the copy you actually use)

```sh
./script/install_local          # build, then install /Applications/Shellomancy.app
./script/install_local --open   # ...and launch it
```

Every bundle build leaves a `Shellomancy.app` under `target/` (`osx/` and `dmg/`, per profile), all with the same bundle id. macOS opens whichever one Launch Services prefers, which is often a stale one. On 2026-11-14 that was a half-finished release build from the night before. `install_local` builds the debug bundle, copies it to `/Applications/Shellomancy.app` (override the folder with `SHELLOMANCY_INSTALL_DIR`), deletes every `Shellomancy.app` under `target/`, and makes Launch Services forget every other registered copy. If the `shellomancy` command is installed, its symlink is pointed at the installed app. It refuses to run while Shellomancy is open, because replacing a running app's bundle breaks it. `--skip-build` installs a build made earlier with `script/bundle`.

Only one install builds at a time, and the newest wins: starting `install_local` stops an older one that's still building (with its cargo and rustc). The build runs at low priority (`nice`) with 6 parallel jobs (override with `CARGO_BUILD_JOBS`) so the machine stays usable, and after a successful install it deletes incremental-compile caches older than a week. Other cargo commands on the same `target/` already wait their turn on cargo's lock.

Each build is stamped with the time it was made, as `v0.YYYY.MM.DD.HH.MM.local_00` (UTC), in its Info.plist only (`script/macos/bundle --version-tag`). The app reads it at runtime, so the tag changing every build doesn't recompile the crates above `warp_core`. The About page shows it, and Check for updates compares it with the published releases: a release made after the build replaces it, if both are signed with the same identity.

`target/` grows large (145 GB on 2026-11-18, about half of it stale incremental caches and old crate builds). `cargo clean` reclaims all of it at the cost of a full rebuild.

It installs a debug build, but Warp's dogfood features (memory stats under every block, debug menus) stay off on Shellomancy unless you launch with `SHELLOMANCY_DEBUG=1`, e.g. `SHELLOMANCY_DEBUG=1 open /Applications/Shellomancy.app`.

## Faster loops

Type-check without Metal (the empty-shader escape hatch is upstream's):

```sh
WARP_LOCAL_SKIP_METAL=1 cargo check -p warp --features gui,warp_control_cli --bin warp-oss
```

This took about 5 minutes cold on the build machine and is the check used for every Shellomancy commit so far. The app it would produce has no shaders, so don't run it.

Tests used for Shellomancy changes (all with `WARP_LOCAL_SKIP_METAL=1`):

```sh
cargo test -p warp_core --features local_fs
cargo test -p local_control
cargo test -p warp_cli
cargo test -p warp --lib -- settings_view:: local_control:: settings::local_control
python3 -m unittest discover -s script/shellomancy_client
```

`cargo test -p warp_completer` currently fails; see [known-issues.md](known-issues.md).

## Release bundle

```sh
brew install create-dmg      # the bundle script always builds a DMG
./script/bundle --channel oss --selfsign --nouniversal
```

This uses the `release-lto` profile and the `release_bundle` feature. `release_bundle` turns on Warp's auto-update, changelog, crash-reporting, handoff-checkpoint and SSH-remote-server flags. Shellomancy forces the others off again at startup (`OFFLINE_DISABLED_FEATURES`); auto-update stays on, pointed at our GitHub releases (see [Releasing](#releasing)). Not verified.

## Releasing

```sh
brew install create-dmg
./script/install_cargo_release_deps   # cargo-about, for the licenses file
./script/release                      # build, check, draft a GitHub release
./script/release --publish            # ...or publish it straight away
```

`script/release` needs a clean, pushed commit. It tags the build with the UTC time (`v0.2026.11.19.17.05.oss_00`), builds an Apple Silicon release bundle, checks that the app is signed (not ad hoc) and carries that version, then uploads three assets: `Shellomancy-arm64.dmg`, `Shellomancy-arm64.dmg.sha256` and `channel_versions.json`. A draft is invisible to updaters until it's published. `--dry-run` stops before uploading. It has not been run yet.

How an installed copy updates: Check for updates (or the automatic check, off by default) reads `releases/latest/download/channel_versions.json`. If that version is newer, it downloads the DMG, checks it against the `.sha256`, and installs it only if the new app is signed the same way as the running one (its designated requirement). So **every release has to be signed with the same identity**, and an ad hoc build can't update at all.

To move to an Apple Developer ID later without stranding installed copies, build one release with `SHELLOMANCY_RELEASE_TEAM_ID=<team id>` set while still signing with the old identity: copies built from it also trust releases signed by that team. A copy built before then accepts only its own signer, so it would need one manual reinstall.

## Signing and the Keychain

Without an Apple Development certificate the app is ad-hoc signed. Each rebuild changes the signature, so macOS asks again for Keychain access to each Shellomancy item it reads at startup: `MadnessAccount` (your sign-in) and `FileBasedMcpCredentials`. Two prompts every launch of a new build (seen 2026-11-15).

Fix it once:

```sh
./script/setup_local_signing
```

This creates a self-signed code-signing identity, "Shellomancy Local Signing", in your login keychain. When no Apple Development certificate exists, `script/macos/bundle --selfsign` (and so `install_local`) signs with it instead of ad hoc, so every build has the same signature. Answer the first Keychain prompts after that with **Always Allow** and they stay answered across rebuilds. The first signing also asks once for codesign to use the new key; answer that with Always Allow too. To undo, delete the identity in Keychain Access.
