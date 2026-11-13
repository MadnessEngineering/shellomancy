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

Write a mapping file (key, standard wording, shellomancer wording, and which
Rust literals it replaces) and run the sweep tool; its header documents the
format:

```sh
script/voice_sweep.py sweeps/settings.json --dry-run   # checks every count
script/voice_sweep.py sweeps/settings.json             # voices + Rust in one go
```

It writes the keys into `standard`, `shellomancer` and `debug`, copies
standard's wording to the other voices (so they read as owed a pass), checks
parity, and swaps each literal for `madness_themes::t("…")`. A `const`, a
`format!` template, or a string that is also an identifier needs a hand edit
afterwards; `cargo check -p warp` finds them. Strings with values use
`{{name}}` placeholders and `madness_themes::tf(key, &[("name", value)])`.
Then commit and push `madness-themes`, and commit here with the bump.

**Standard** keeps plain nouns (Shellomancy, Drive, block, pane, shell
integration); **shellomancer** uses the lexicon (Mana Bar, spell, chamber,
Enchant). Only reachable text is reworded: features that need Warp's servers
are hidden instead. The About page's "A fork of Warp" credit line stays.

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
Each one is a built-in theme (`ThemeKind::Madness("<palette id>")`), listed
first in the theme picker, ahead of Warp's own themes, which stay. Its colours are
the palette's `terminal` block, read through the same deserializer as a custom
theme file. To change one, edit the palette in MadnessThemes (`palettetool.py`
explains the derivation and checks contrast) and bump the submodule.
