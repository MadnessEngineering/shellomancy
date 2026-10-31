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

This uses the `release-lto` profile and the `release_bundle` feature. `release_bundle` turns on Warp's auto-update, changelog, crash-reporting, handoff-checkpoint and SSH-remote-server flags, but Shellomancy forces those off again at startup (`OFFLINE_DISABLED_FEATURES`), so a release bundle stays offline. Not verified.

## Signing and the Keychain

Without an Apple Development certificate the app is ad-hoc signed. Each rebuild changes the signature, so macOS may ask again for Keychain access to Shellomancy's items (its local-control setting lives there). Expected, not verified.
