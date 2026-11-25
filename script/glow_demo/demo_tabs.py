#!/usr/bin/env python3
"""Opens `make dev-glow-demo`'s pretend Claude tabs, one per glow, in the first window of the
Shellomancy that WARP_LOCAL_CONTROL_DISCOVERY_DIR points at (the dev copy). The tab ids go in
RECORD so the next run closes these tabs before opening new ones, instead of piling them up.

    demo_tabs.py RECORD
"""

import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "shellomancy_client"))
import shellomancy_client as client  # noqa: E402

STATES = ["working", "needs-you", "done", "failed"]


def main(record: Path) -> int:
    window = client.send("window.list")["windows"][0]["window_id"]
    previous = record.read_text().split() if record.exists() else []
    if previous:
        # The pretend sessions are running commands; ending them first lets their tabs close
        # without a "processes are running" prompt.
        subprocess.run(["pkill", "-f", str(HERE / "claude")], check=False)
        for tab_id in previous:
            try:
                client.send(
                    "tab.close",
                    {"mode": "target"},
                    target={"window": {"type": "id", "id": window}, "tab": {"type": "id", "id": tab_id}},
                )
            except client.ShellomancyError:
                pass  # already closed
    opened = []
    for state in STATES:
        data = client.open_tab(
            command=f"PATH={HERE}:$PATH claude {state}",
            title=f"glow: {state}",
            focus=False,
            window_id=window,
        )
        opened.append(data["tab"]["id"])
        print(f"Opened a '{state}' tab.")
    record.parent.mkdir(parents=True, exist_ok=True)
    record.write_text("\n".join(opened) + "\n")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    sys.exit(main(Path(sys.argv[1])))
