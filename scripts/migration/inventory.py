#!/usr/bin/env python3
"""Inventory the Go backend and write MIGRATION.md.

Extracts every non-test top-level function and method, the package import
graph, and an approximate intra-package call graph. Packages are emitted
leaf-first and functions inside a package are ordered by the call graph so
callees come before callers.

Run from the repository root:

    python3 scripts/migration/inventory.py
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BACKEND = ROOT / "backend"
DESKTOP = ROOT / "desktop"
OUT = ROOT / "MIGRATION.md"

FUNC_RE = re.compile(
    r"^func\s+(?:\((?P<recv>[^)]*)\)\s+)?(?P<name>[A-Za-z0-9_]+)(?:\[[^\]]*\])?\s*\("
)
IMPORT_RE = re.compile(r'"azahar/internal/([^"]+)"')


@dataclass
class Func:
    pkg: str
    name: str
    recv: str
    file: str
    line: int
    body: str = ""
    calls: set[str] = field(default_factory=set)

    @property
    def label(self) -> str:
        if self.recv:
            return f"{self.name}"
        return self.name


def packages() -> dict[str, tuple[Path, str]]:
    """Package key -> (directory, Go import prefix)."""
    found: dict[str, tuple[Path, str]] = {}
    for go_mod in BACKEND.rglob("*.go"):
        if go_mod.name.endswith("_test.go"):
            continue
        rel = go_mod.parent.relative_to(BACKEND)
        found[str(rel)] = (go_mod.parent, "azahar/")
    for go_mod in DESKTOP.rglob("*.go"):
        if go_mod.name.endswith("_test.go"):
            continue
        rel = go_mod.parent.relative_to(DESKTOP)
        key = "desktop" if str(rel) == "." else f"desktop/{rel}"
        found[key] = (go_mod.parent, "azahar/desktop/")
    return found


def extract_funcs(pkg: str, directory: Path) -> list[Func]:
    funcs: list[Func] = []
    for path in sorted(directory.glob("*.go")):
        if path.name.endswith("_test.go"):
            continue
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        i = 0
        while i < len(lines):
            match = FUNC_RE.match(lines[i])
            if not match:
                i += 1
                continue
            name = match.group("name")
            recv = (match.group("recv") or "").strip()
            start = i
            depth = 0
            started = False
            body_lines: list[str] = []
            while i < len(lines):
                line = lines[i]
                body_lines.append(line)
                depth += line.count("{") - line.count("}")
                if "{" in line:
                    started = True
                if started and depth <= 0:
                    break
                i += 1
            funcs.append(
                Func(
                    pkg=pkg,
                    name=name,
                    recv=recv,
                    file=str(path.relative_to(ROOT)),
                    line=start + 1,
                    body="\n".join(body_lines),
                )
            )
            i += 1
    return funcs


def package_imports(directory: Path, prefix: str) -> set[str]:
    pattern = re.compile('"' + re.escape(prefix) + '([^"]+)"')
    deps: set[str] = set()
    for path in directory.glob("*.go"):
        if path.name.endswith("_test.go"):
            continue
        deps.update(pattern.findall(path.read_text(encoding="utf-8", errors="replace")))
    return deps


def resolve_dep(dep: str, dirs: dict[str, Path]) -> str | None:
    """Map an import suffix such as `store` to a package key."""
    for key in (dep, f"internal/{dep}", f"desktop/{dep}"):
        if key in dirs:
            return key
    return None


def order_packages(deps: dict[str, set[str]], dirs: dict[str, Path]) -> list[str]:
    ordered: list[str] = []
    seen: set[str] = set()
    visiting: set[str] = set()

    def visit(pkg: str) -> None:
        if pkg in seen or pkg in visiting:
            return
        visiting.add(pkg)
        for dep in sorted(deps.get(pkg, set())):
            resolved = resolve_dep(dep, dirs)
            if resolved is not None:
                visit(resolved)
        visiting.remove(pkg)
        seen.add(pkg)
        ordered.append(pkg)

    for pkg in sorted(deps):
        visit(pkg)
    return ordered


def order_funcs(funcs: list[Func]) -> list[Func]:
    # Several methods can share a name (Close, Manifest...). Key call edges by
    # name, but de-duplicate output by source location.
    by_name: dict[str, list[Func]] = {}
    for func in funcs:
        by_name.setdefault(func.name, []).append(func)
    for func in funcs:
        for called, targets in by_name.items():
            if called == func.name:
                continue
            if re.search(r"\b" + re.escape(called) + r"\s*\(", func.body):
                func.calls.add(called)

    ordered: list[Func] = []
    seen: set[tuple[str, int]] = set()

    def visit(func: Func) -> None:
        key = (func.file, func.line)
        if key in seen:
            return
        seen.add(key)
        for dep in sorted(func.calls):
            for target in by_name[dep]:
                visit(target)
        ordered.append(func)

    for func in funcs:
        visit(func)
    return ordered


def main() -> int:
    if not BACKEND.is_dir():
        print("backend/ not found", file=sys.stderr)
        return 2

    dirs = packages()
    deps = {pkg: package_imports(directory, prefix) for pkg, (directory, prefix) in dirs.items()}
    ordered = order_packages(deps, dirs)
    all_funcs = {pkg: extract_funcs(pkg, dirs[pkg][0]) for pkg in dirs}
    counts = {pkg: len(funcs) for pkg, funcs in all_funcs.items()}
    line_counts: dict[str, int] = {}
    for pkg, (directory, _prefix) in dirs.items():
        total = 0
        for path in directory.glob("*.go"):
            if path.name.endswith("_test.go"):
                continue
            total += len(path.read_text(encoding="utf-8", errors="replace").splitlines())
        line_counts[pkg] = total

    total_funcs = sum(counts.values())

    out: list[str] = []
    out.append("# Azahar Go → Rust migration")
    out.append("")
    out.append(
        "Living plan for the Rust port. The Go tree stays runnable and untouched as the"
        " **oracle**; every Rust function is diff-tested against it before its box is ticked."
    )
    out.append("")
    out.append("## Scope decisions")
    out.append("")
    out.append(
        "- **In scope:** every Go package under `backend/` (the API, store, plugins, courier,"
        " seed and CLI) and the Go host logic under `desktop/`."
    )
    out.append(
        "- **Out of scope:** `frontend/` (SvelteKit + TypeScript). It is the UI layer, not a"
        " second implementation of server logic; the Rust API keeps its HTTP contract so the"
        " same frontend runs against it. Porting Svelte components to Rust is not a language"
        " migration of logic and would destroy the working UI. Logged as decision D1."
    )
    out.append(
        "- **Oracle rule:** no file under `backend/` is deleted or modified. Tooling added next"
        " to it (for example a `cmd/oracle` harness) is additive and does not change behaviour."
    )
    out.append(
        "- **Function unit:** each checkbox is one Go function or method (non-test). Tests are"
        " not ported one-to-one; equivalent Rust tests and the differential harness replace them."
    )
    out.append(
        "- **Migration branch:** `rust-migrate`. `main` is never committed to from this work."
    )
    out.append("")
    out.append("## Orientation")
    out.append("")
    out.append(f"- Go packages: **{len(dirs)}**, non-test functions/methods: **{total_funcs}**.")
    out.append(
        "- Rust workspace crates mirror each Go package (`crates/azahar-<package>`), with"
        " `azahar-api` as the binary."
    )
    out.append("- Package dependency order (leaf-first):")
    out.append("")
    out.append("```text")
    for pkg in ordered:
        resolved = sorted(r for d in deps.get(pkg, set()) if (r := resolve_dep(d, dirs)))
        deps_text = ", ".join(resolved) or "—"
        out.append(f"{pkg:<20} <- {deps_text}")
    out.append("```")
    out.append("")
    out.append("## Package summary")
    out.append("")
    out.append("| Package | Functions | Go LOC | Depends on |")
    out.append("| --- | ---: | ---: | --- |")
    for pkg in ordered:
        resolved = sorted(r for d in deps.get(pkg, set()) if (r := resolve_dep(d, dirs)))
        dep_text = ", ".join(resolved) or "—"
        out.append(f"| `{pkg}` | {counts[pkg]} | {line_counts[pkg]} | {dep_text} |")
    out.append("")
    out.append("## Subagent log")
    out.append("")
    out.append("| # | Role | Assignment | Status | Outcome |")
    out.append("| ---: | --- | --- | --- | --- |")
    out.append(
        "| 1 | architect | call graph, crate layout, ordering | in progress | produced this inventory |"
    )
    out.append("")
    out.append("Status values: `queued`, `running`, `review`, `done`, `failed`, `rebriefed`.")
    out.append("")
    out.append("## Known behaviour differences")
    out.append("")
    out.append("Differences are only acceptable when behaviour is unobservable at the HTTP/API"
               " boundary or is a documented security improvement. Each entry needs a justification.")
    out.append("")
    out.append("| Item | Difference | Justification |")
    out.append("| --- | --- | --- |")
    out.append("| — | — | — |")
    out.append("")
    out.append("## Checklist")
    out.append("")
    for pkg in ordered:
        out.append(f"### `{pkg}` ({counts[pkg]} functions)")
        out.append("")
        for func in order_funcs(all_funcs[pkg]):
            suffix = " *(method)*" if func.recv else ""
            out.append(f"- [ ] `{pkg}.{func.name}`{suffix} — `{func.file}:{func.line}`")
        out.append("")

    OUT.write_text("\n".join(out) + "\n", encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(dirs)} packages, {total_funcs} functions")

    manifest = {
        "packages": [
            {
                "package": pkg,
                "go_loc": line_counts[pkg],
                "deps": sorted(r for d in deps.get(pkg, set()) if (r := resolve_dep(d, dirs))),
                "functions": [
                    {
                        "name": f.name,
                        "receiver": f.recv,
                        "file": f.file,
                        "line": f.line,
                        "calls": sorted(f.calls),
                    }
                    for f in order_funcs(all_funcs[pkg])
                ],
            }
            for pkg in ordered
        ]
    }
    (ROOT / "MIGRATION.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
