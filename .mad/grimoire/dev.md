# Shellomancy dev loop

Build, check, test, ship. From the repo root. The long version is
`docs/building.md`; voices are `docs/voice.md`.

## Install the copy you use

`install_local` refuses to run while Shellomancy is open (replacing a running
app's bundle breaks it), so run this from another terminal, then reopen.

```sh
./script/install_local --open
```

## Check fast

```sh
cargo check -p warp
```

The app's own binary with its real features, skipping Metal shader compiles:

```sh
WARP_LOCAL_SKIP_METAL=1 cargo check -p warp --features gui,warp_control_cli --bin warp-oss
```

## Test

Run tests bare and read cargo's exit code. Piping through `tail`, `head` or
`grep` reports the filter's exit code instead (a guard hook blocks it anyway).

The Madness parts (Grimoire, tools panel tabs, search scopes, Mana Bar):

```sh
cargo test -p warp --lib -- grimoire test_tools_panel left_panel app_state global_search mana_bar
```

```sh
cargo test -p madness_themes
```

Others from `docs/building.md`:

```sh
cargo test -p warp_core --features local_fs && cargo test -p local_control && cargo test -p warp_cli
```

`cargo test -p warp_completer` fails today (see `docs/known-issues.md`).

## Format what you touched

```sh
git diff --name-only -- '*.rs' | xargs rustfmt --edition 2024
```

## Words (voices)

New UI text goes through a sweep file, never a literal. Write
`script/sweeps/NN-name.json` (key, standard, shellomancer), then:

```sh
script/voice_sweep.py script/sweeps/NN-name.json --dry-run
```

```sh
script/voice_sweep.py script/sweeps/NN-name.json
```

Then commit and push `madness-themes` itself, and commit the bump here:

```sh
(cd madness-themes && git add -u && git commit -m "STRINGS FOR WHAT" && git push) && git add madness-themes
```

## Watch it run

```sh
tail -f ~/Library/Logs/shellomancy.log
```

Debug features stay off unless `SHELLOMANCY_DEBUG` is set.

## Release (not run yet)

Needs a clean, pushed commit. `--dry-run` stops before uploading.

```sh
./script/release --dry-run
```
