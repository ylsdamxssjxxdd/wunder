"""Fail when a ScrollView cannot infer its content height.

Slint's compiler derives `content-height` only from *layout* children
(`i-slint-compiler`, flickable pass). A component or a bare rectangle child
leaves `content-height` at the viewport height, so the view silently clips its
content: no scrollbar, no wheel movement, and nothing in the build complains.
That is how a 40-line tool output ended up showing only its middle.

Rule enforced here: every `ScrollView` must either

  * declare `content-height:` itself, or
  * have a layout (VerticalLayout / HorizontalLayout / GridLayout) as its first
    child, which the compiler can measure.

Usage:
    python frontend-slint/scripts/check-scrollview-content.py [--fix-hint]

Exit code 1 lists the offenders with a pointer to the pattern to copy
(`help_document.slint`, `timeline.slint` transcript scroll).
"""
import argparse
import re
import sys
from pathlib import Path

UI = Path(__file__).resolve().parents[1] / "ui"
LAYOUTS = {"VerticalLayout", "HorizontalLayout", "GridLayout"}
SCROLL = re.compile(r"(?<![\w-])ScrollView\s*\{")


def block_end(text, start):
    """Index just past the brace-matched block opened at `start`."""
    depth = 0
    for index in range(start, len(text)):
        char = text[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return index + 1
    return len(text)


def first_child(block):
    """Name of the first element declared directly inside the block, or None.

    Property assignments (`x: 10px;`) are skipped because they start with a
    lower-case name; the first `Element {` at brace depth 0 is the child the
    compiler measures. Several ScrollViews in this UI keep their child on the
    same line as the opening brace, so the whole body is scanned rather than
    line by line.
    """
    body = block[block.index("{") + 1:]
    match = re.search(r"(?<![\w.-])([A-Z][\w-]*)\s*\{", body)
    if match and body.count("{", 0, match.start()) == body.count("}", 0, match.start()):
        return match[1]
    return None


def scan(path):
    text = path.read_text(encoding="utf-8")
    findings = []
    for match in SCROLL.finditer(text):
        brace = text.index("{", match.start())
        block = text[match.start():block_end(text, brace)]
        if re.search(r"(?<![\w-])content-height\s*:", block):
            continue
        child = first_child(block)
        if child in LAYOUTS:
            continue
        line = text.count("\n", 0, match.start()) + 1
        findings.append((line, child or "?"))
    return findings


def main():
    parser = argparse.ArgumentParser(description="ScrollView content-height guard")
    parser.add_argument("--ui", type=Path, default=UI, help="directory of .slint files to scan")
    parser.add_argument("--fix-hint", action="store_true", help="print the snippet to add")
    args = parser.parse_args()
    failures = 0
    for path in sorted(args.ui.glob("*.slint")):
        for line, child in scan(path):
            failures += 1
            print(f"{path.name}:{line}: ScrollView child `{child}` is not a layout and no "
                  f"content-height is declared — it will clip, with no scrollbar.")
            if args.fix_hint:
                print("    content-height: max(self.visible-height, <child>.preferred-height);")
    if failures:
        print(f"\n{failures} ScrollView(s) cannot infer their content height "
              f"(see ui/help_document.slint for the pattern).")
        return 1
    print("PASS every ScrollView declares or inherits a measurable content height")
    return 0


if __name__ == "__main__":
    sys.exit(main())
