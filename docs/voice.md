# Voices and palettes

Shellomancy's words and colours come from
[MadnessThemes](https://github.com/MadnessEngineering/MadnessThemes), the
workshop's theme collection (Inventorium and madnessDesktop use it too). It is a
git submodule at `madness-themes/`; after cloning, run:

```sh
git submodule update --init madness-themes
```

The build fails with that same instruction if it's missing.

## Voices: the UI text

Every MadnessThemes voice (`madness-themes/<voice>.json`) has a `shellomancy`
section, `shellomancy.<area>.<element>`, holding Shellomancy's own strings. The
`madness_themes` crate (`crates/madness_themes`) compiles those sections in at
build time, and code asks for a string by key:

```rust
ActionButton::new(madness_themes::t("shellomancy.plugin.update"), theme)
```

`t()` answers from the active voice, then `standard`, then gives the key back.
Pick the voice in **Settings › Appearance › Voice** (or `appearance.voice` in
the settings file); it's read once at startup, so a change applies after a
restart.
The default voice is **shellomancer**: practical magic, dryly delivered (spells,
chambers, the Mana Bar, familiars). Its lexicon, tone rules and examples are its
voice card in
[`VOICE_GUIDE.md`](https://github.com/MadnessEngineering/MadnessThemes/blob/main/VOICE_GUIDE.md#shellomancer---card-written-file-seeded-from-mad-wizard);
`standard` is plain English; `debug` shows each string's key.

### Moving a string off Warp's literal

1. In MadnessThemes, add the key to `standard` (plain), `shellomancer`, and
   `debug` (its own path), then copy it to the other voices:

   ```sh
   ./themetool.py patch standard keys.json      # likewise shellomancer, debug
   ./themetool.py sync standard banana biomedical corporate-clean corporate-drone cyan-lab dwarf gunmetal labops mad-wizard templar-light
   ./themetool.py parity --strict
   ```

2. Commit and push MadnessThemes, then bump `madness-themes` here.
3. Replace the literal with `madness_themes::t("shellomancy.…")`.

`cargo test -p madness_themes` fails if a voice is missing a key, if `debug`
doesn't echo its keys, or if any `shellomancy` string says "Warp".

Only user-visible text moves. Type names, settings keys, telemetry names and
log lines keep Warp's names, so [upstream syncs](upstream-sync.md) don't
conflict on them.

### Converted so far

- The agent-plugin footer chip and its toasts (`shellomancy.plugin.*`).
- The Voice picker's own labels (`shellomancy.settings.voice.*`).

## Palettes: terminal colours

Every MadnessThemes palette (`*-colors.json`) carries a `terminal` block:
background, foreground, accent, cursor, and the 16 ANSI colours.
`madness_themes::palettes()` exposes them; they are not in the theme picker yet.
