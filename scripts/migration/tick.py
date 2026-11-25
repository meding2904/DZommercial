#!/usr/bin/env python3
"""Tick MIGRATION.md checkboxes for one package, or one function.

    python3 scripts/migration/tick.py internal/auth
    python3 scripts/migration/tick.py internal/auth HashPassword VerifyPassword

Without function names, every checkbox in the package section is ticked (use
only after the package's differential tests are green).
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / "MIGRATION.md"


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    package = sys.argv[1]
    names = set(sys.argv[2:])
    lines = PATH.read_text(encoding="utf-8").splitlines()
    in_section = False
    ticked = 0
    for index, line in enumerate(lines):
        if line.startswith("### `"):
            in_section = line.startswith(f"### `{package}` ")
            continue
        if not in_section or not line.startswith("- [ ] `"):
            continue
        match = re.match(r"- \[ \] `([^`]+)`", line)
        if not match:
            continue
        entry = match.group(1)
        function = entry.rsplit(".", 1)[-1]
        if names and function not in names:
            continue
        lines[index] = line.replace("- [ ]", "- [x]", 1)
        ticked += 1
    if not ticked:
        print(f"no checkboxes ticked for {package} {sorted(names)}")
        return 1
    PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"ticked {ticked} in {package}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
