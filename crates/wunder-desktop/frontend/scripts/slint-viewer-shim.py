#!/usr/bin/env python3
"""Run the stock ``slint-viewer`` against this project's Slint sources.

The application is built with the vendored Slint fork (see the ``[patch]``
entries in ``Cargo.toml``).  That fork adds a ``context-menu`` callback to the
built-in ``StyledText`` element and gates ``index-of`` behind
``SLINT_ENABLE_EXPERIMENTAL_FEATURES``.  The stock viewer binary knows neither,
so ``slint-viewer --check`` fails on four handlers in ``markdown-rich.slint``
and on the settings dropdowns even though ``cargo check`` succeeds.

This script builds a throwaway copy of ``ui/`` in which only the four
fork-only ``StyledText`` handlers are dropped (right-click menus still work in
the real app because those bindings live in the untouched source tree), copies
the image assets the sheets reference, enables the experimental flag, and then
forwards every argument to ``slint-viewer`` with the shim as its working
directory.

Usage:
    python scripts/slint-viewer-shim.py --check ui/main.slint
    python scripts/slint-viewer-shim.py --screenshot ../axis.png \
        --size 1100x900 --component ThreadTrajectoryPanel ui/thread_trajectory.slint

Set ``SLINT_VIEWER_SHIM_DIR`` to move the generated copy elsewhere.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
UI = ROOT / "ui"
ASSETS = ROOT / "assets"
OUT = Path(
    os.environ.get(
        "SLINT_VIEWER_SHIM_DIR", str(ROOT.parent / "temp_dir" / "viewer-shim")
    )
)
SHIM_UI = OUT / "ui"

# Matches a handler bound *inside* a StyledText element.  Handlers bound on
# locally declared components (ProseBlock, CodeBlock, ...) are legal in both
# compilers and must be preserved, so the removal is scoped to StyledText
# blocks by brace depth rather than applied file-wide.
STYLED_TEXT_OPEN = re.compile(r"\bStyledText\s*\{")
FORK_HANDLER = re.compile(r"^\s*context-menu\([^)]*\)\s*=>")
ASSET_REF = re.compile(r"\.\./assets/[^'\"\s)]+")


def shim_sheet(text: str) -> tuple[str, int]:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    depth = 0
    in_block = False
    removed = 0
    for line in lines:
        if not in_block and STYLED_TEXT_OPEN.search(line):
            in_block = True
            depth = line.count("{") - line.count("}")
            out.append(line)
            continue
        if in_block:
            depth += line.count("{") - line.count("}")
            if FORK_HANDLER.match(line):
                removed += 1
            else:
                out.append(line)
            if depth <= 0:
                in_block = False
            continue
        out.append(line)
    return "".join(out), removed


def build() -> int:
    SHIM_UI.mkdir(parents=True, exist_ok=True)
    removed_total = 0
    refs: set[str] = set()
    for source in sorted(UI.rglob("*.slint")):
        target = SHIM_UI / source.relative_to(UI)
        target.parent.mkdir(parents=True, exist_ok=True)
        text = source.read_text(encoding="utf-8")
        refs.update(ASSET_REF.findall(text))
        if source.name == "markdown-rich.slint":
            text, removed = shim_sheet(text)
            removed_total += removed
        target.write_text(text, encoding="utf-8", newline="")
    for ref in sorted(refs):
        relative = ref[len("../assets/") :]
        source = ASSETS / relative
        if not source.is_file():
            continue
        target = OUT / "assets" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    return removed_total


def main() -> int:
    removed = build()
    if removed != 4:
        print(
            f"slint-viewer-shim: expected 4 fork-only handlers, removed {removed}; "
            "the compiler fork may have changed",
            file=sys.stderr,
        )
    viewer = shutil.which("slint-viewer")
    if not viewer:
        print("slint-viewer-shim: slint-viewer is not on PATH", file=sys.stderr)
        return 127
    args = sys.argv[1:] or ["--check", "ui/main.slint"]
    env = os.environ.copy()
    # build.rs sets the same flag for the vendored compiler.
    env["SLINT_ENABLE_EXPERIMENTAL_FEATURES"] = "1"
    return subprocess.run([viewer, *args], cwd=OUT, env=env, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
