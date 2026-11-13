#!/usr/bin/env python3
"""voice_sweep — move user-visible strings off Warp's literals into MadnessThemes.

Takes a mapping file and does both halves of a conversion in one go:

  1. writes each key into the voices in madness-themes/ (standard, shellomancer,
     debug from the mapping; every other voice gets standard's words through
     themetool's sync, which marks them as owed a voice pass), and
  2. replaces each Rust literal with `madness_themes::t("<key>")` (or the
     `code` you give, for strings that need tf() or a different shape).

    script/voice_sweep.py sweeps/settings.json            # check, then apply
    script/voice_sweep.py sweeps/settings.json --dry-run  # check only

Mapping file: a JSON list of entries.

    {
      "key": "shellomancy.settings.toolsPanel.drive",
      "standard": "Drive",            # plain English, no "Warp"
      "shellomancer": "Mana Bar",     # the default voice's lexicon
      "replace": [                    # optional: Rust literals to swap
        {"file": "app/src/settings_view/appearance_page.rs",
         "literal": "Warp Drive",     # as written between the quotes
         "count": 1,                  # occurrences expected in that file
         "code": null}                # optional replacement, default t(key)
      ]
    }

Every replacement's count is checked before anything is written, so a mapping
that has gone stale fails cleanly instead of half-applying. Neither standard nor
shellomancer wording may say "Warp". After applying: `cargo check -p warp`,
`cargo test -p madness_themes`, commit and push madness-themes, then commit
here with the submodule bump.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
THEMES = REPO / "madness-themes"
OWN = ("standard", "shellomancer", "debug")
WARP = re.compile(r"\bWarp")


def fail(msg):
    sys.exit(f"voice_sweep: {msg}")


def load(path):
    entries = json.loads(Path(path).read_text())
    keys = set()
    for e in entries:
        key = e.get("key", "")
        if not key.startswith("shellomancy."):
            fail(f"key must start with shellomancy.: {key!r}")
        if key in keys:
            fail(f"duplicate key {key}")
        keys.add(key)
        for voice in ("standard", "shellomancer"):
            text = e.get(voice)
            if not isinstance(text, str) or not text:
                fail(f"{key}: missing {voice} wording")
            if WARP.search(text):
                fail(f"{key}: {voice} wording says Warp: {text!r}")
    return entries


def plan_replacements(entries):
    """[(file, old, new)] after checking every count against the files."""
    plan = []
    texts = {}
    for e in entries:
        for r in e.get("replace", []):
            path = REPO / r["file"]
            if path not in texts:
                if not path.is_file():
                    fail(f"{e['key']}: no such file {r['file']}")
                texts[path] = path.read_text()
            old = f'"{r["literal"]}"'
            new = r.get("code") or f'madness_themes::t("{e["key"]}")'
            found = texts[path].count(old)
            want = r.get("count", 1)
            if found != want:
                fail(f"{e['key']}: {r['file']} has {found} of {old}, mapping expects {want}")
            plan.append((path, old, new))
    return plan


def themetool(*args):
    out = subprocess.run([sys.executable, str(THEMES / "themetool.py"), *args],
                         cwd=THEMES, capture_output=True, text=True)
    if out.returncode:
        fail(f"themetool {' '.join(args)}: {out.stderr or out.stdout}")
    return out.stdout


def write_voices(entries, scratch):
    patches = {
        "standard": {e["key"]: e["standard"] for e in entries},
        "shellomancer": {e["key"]: e["shellomancer"] for e in entries},
        "debug": {e["key"]: e["key"] for e in entries},
    }
    for voice, patch in patches.items():
        f = scratch / f"voice_sweep_{voice}.json"
        f.write_text(json.dumps(patch, ensure_ascii=False))
        print(themetool("patch", voice, str(f)).strip().splitlines()[-1])
    others = sorted(p.stem for p in THEMES.glob("*.json")
                    if not p.stem.endswith("-colors") and p.stem not in OWN)
    themetool("sync", "standard", *others)
    themetool("parity", "--strict")
    print(f"synced standard's words to {len(others)} other voices; parity clean")


def main():
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("mapping")
    p.add_argument("--dry-run", action="store_true")
    args = p.parse_args()

    if not (THEMES / "themetool.py").is_file():
        fail("madness-themes submodule missing: git submodule update --init madness-themes")
    entries = load(args.mapping)
    plan = plan_replacements(entries)
    print(f"{len(entries)} keys, {len(plan)} replacements checked")
    if args.dry_run:
        return

    write_voices(entries, Path(args.mapping).resolve().parent)
    for path, old, new in plan:
        path.write_text(path.read_text().replace(old, new))
    files = sorted({str(path.relative_to(REPO)) for path, _, _ in plan})
    print("rewrote " + ", ".join(files) if files else "no Rust changes")


if __name__ == "__main__":
    main()
