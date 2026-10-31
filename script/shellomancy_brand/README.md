# Shellomancy brand assets

Regenerates every Shellomancy-branded asset from one source:
`app/channels/oss/icon/shellomancy.svg` (the app icon) plus the nautilus
geometry in `mark.py`.

| Script | Writes |
|---|---|
| `build.py` | `app/assets/bundled/svg/warp*.svg` (tinted marks, 12 loading frames, logos, logo + wordmark), `app/assets/bundled/png/{blue,dev,local}.png`, `app/DockTilePlugin/Resources/*.png` (Default + Abyss/Bone/Ember/Mono/Toxic palettes) |
| `font_glyph.py` | the `warpLogo` glyph (U+E500) in `app/assets/bundled/fonts/roboto/*.ttf` |

File names stay `warp*` so upstream code that references them is unchanged.

Requirements: `rsvg-convert` (`brew install librsvg`) and `fonttools`
(`python3 -m venv .venv && .venv/bin/pip install fonttools`).

```
.venv/bin/python script/shellomancy_brand/build.py "$PWD"
.venv/bin/python script/shellomancy_brand/font_glyph.py "$PWD"
```

`build.py` deletes and rewrites every PNG in `app/DockTilePlugin/Resources`.
`font_glyph.py` fits the glyph into the original Warp glyph's box
(85,0)-(1963,1502) and keeps its 2048-unit advance; re-running is idempotent.
