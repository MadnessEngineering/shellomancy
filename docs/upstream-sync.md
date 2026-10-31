# Keeping up with upstream Warp

Shellomancy drifts from Warp on purpose; it is not trying to stay mergeable as a pull request. It does want Warp's terminal improvements, so it pulls upstream periodically.

## Remotes

```sh
git remote -v
# origin    git@github.com:MadnessEngineering/shellomancy.git
# upstream  https://github.com/warpdotdev/warp.git
```

The fork point is `d57d9760`. Work happens on `madness/init-shellomancy`.

## Pulling upstream

```sh
git fetch upstream
git merge upstream/master
```

Merge rather than rebase: the history then shows exactly which upstream range each catch-up brought in. Do big catch-ups before large Shellomancy features, not in the middle of one.

## Keeping conflicts small

- **Prefer switches to deletions.** Turn features off with flags (`OFFLINE_DISABLED_FEATURES`), channel config (`app/src/bin/oss.rs`) or a guarded early return, instead of deleting upstream code. A deletion conflicts with every upstream edit to that code; a guard usually doesn't.
- **Mark every change.** Each Shellomancy change in code carries a `Shellomancy:` comment. After a merge, `rg "Shellomancy:"` shows what the fork changed and makes a lost change easy to spot.
- **Keep identity in one place.** Names live in `crates/warp_core/src/channel/` (`Channel::Oss` arms, `config.rs`), `crates/warp_core/src/paths.rs`, `app/src/bin/oss.rs`, `app/Cargo.toml` bundle metadata, and the scripts. The binary keeps upstream's name `warp-oss` to avoid touching more scripts.
- **Regenerate brand assets instead of hand-merging them.** If upstream changes a `warp*.svg`, a Dock icon or the Roboto fonts, take theirs and re-run [`script/shellomancy_brand`](../script/shellomancy_brand/README.md).

## Where conflicts are likely

These files change often upstream and also carry Shellomancy changes:

- `crates/warp_core/src/paths.rs`, `crates/warp_core/src/channel/*`
- `app/src/bin/oss.rs`, `app/Cargo.toml`
- `script/run`, `script/macos/run`, `script/macos/bundle`, `script/update_plist`
- `app/src/local_control/**`, `crates/local_control/**`, `crates/warp_cli/src/local_control/**`
- `app/src/settings_view/mod.rs`, `app/src/workspace/{mod,view}.rs`, `app/src/lib.rs`
- `app/src/settings/app_icon.rs`, `app/DockTilePlugin/*`

## After a merge

1. `rg "Shellomancy:"` — every marker should still be present and make sense.
2. Check for new upstream feature flags that reach Warp's servers or belong to the Oz platform, and add them to `OFFLINE_DISABLED_FEATURES`.
3. Run the checks in [building.md](building.md#faster-loops).
4. If upstream added Warp-branded assets, regenerate with `script/shellomancy_brand`.
5. Update [differences-from-warp.md](differences-from-warp.md) and [known-issues.md](known-issues.md) in the same commit as any behavior change.
