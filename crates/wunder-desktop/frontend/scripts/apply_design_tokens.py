"""One-off migration: rewrite UI literals to design tokens (N1).

Safe replacements only:
- exact font-size / border-radius scale points,
- status hex colors and modal scrims outside quoted strings,
- shell layout metrics in their exact page-root / rail / middle / dock
  contexts (composer popups and square boxes keep their literals).
design_tokens.slint and screenshot_overlay.slint are excluded.
"""
import glob
import re

SCALE = {
    "font-size": {"10": "font-2xs", "11": "font-xs", "12": "font-sm", "13": "font-md",
                  "14": "font-lg", "16": "font-xl", "18": "font-headline", "24": "font-display"},
    "border-radius": {"7": "radius-sm", "8": "radius-sm", "9": "radius-md", "10": "radius-md",
                      "11": "radius-lg", "12": "radius-lg", "14": "radius-xl"},
}
COLORS = {
    "#b63a22": "Theme.danger",
    "#16845d": "Theme.success",
    "#b8860b": "Theme.warning",
    "#3b82f6": "Theme.info",
    "#00000022": "Theme.overlay",
    "#00000030": "Theme.overlay",
    "#00000033": "Theme.overlay",
    "#00000035": "Theme.overlay",
    "#00000040": "Theme.overlay",
    "#00000044": "Theme.overlay",
}
EXCLUDED = {"ui/design_tokens.slint", "ui/screenshot_overlay.slint"}


def sub_outside_quotes(line, pattern, repl):
    parts = line.split('"')
    for index in range(0, len(parts), 2):
        parts[index] = pattern.sub(repl, parts[index])
    return '"'.join(parts)


def main():
    changed = 0
    for path in sorted(glob.glob("ui/*.slint")):
        if path.replace("\\", "/") in EXCLUDED:
            continue
        source = open(path, encoding="utf-8").read()
        original = source
        for prop, mapping in SCALE.items():
            for size, token in mapping.items():
                source = re.sub(
                    rf"{prop}:\s*{size}px\b",
                    f"{prop}: Theme.{token}",
                    source,
                )
        for line_index, line in enumerate(source.splitlines(keepends=True)):
            for hex_value, token in COLORS.items():
                line = sub_outside_quotes(
                    line, re.compile(re.escape(hex_value) + r"\b"), token
                )
            source_lines = source.splitlines(keepends=True)
            source_lines[line_index] = line
            source = "".join(source_lines)
        if path.replace("\\", "/") == "ui/main.slint":
            source = source.replace(
                "x: root.navigation-open ? 276px : 56px;",
                "x: root.navigation-open ? Theme.rail-width + Theme.middle-width : Theme.rail-width;",
            ).replace(
                "x: (root.navigation-open ? 276px : 56px) - 4px;",
                "x: (root.navigation-open ? Theme.rail-width + Theme.middle-width : Theme.rail-width) - 4px;",
            ).replace(
                "x: 0px; y: 0px; width: 56px; height: parent.height; background: Theme.rail;",
                "x: 0px; y: 0px; width: Theme.rail-width; height: parent.height; background: Theme.rail;",
            ).replace(
                "x: 56px; y: 0px; width: 220px; height: parent.height; background: Theme.middle;",
                "x: Theme.rail-width; y: 0px; width: Theme.middle-width; height: parent.height; background: Theme.middle;",
            ).replace(
                "284px", "Theme.dock-width"
            ).replace(
                "1100px", "Theme.dock-breakpoint"
            ).replace(
                "x: 56px; y: 0px; width: parent.width - 56px;",
                "x: Theme.rail-width; y: 0px; width: parent.width - Theme.rail-width;",
            )
        else:
            # Middle-column panes: 220px immediately followed by the middle brush.
            source = source.replace(
                "width: 220px;\n            background: Theme.middle;",
                "width: Theme.middle-width;\n            background: Theme.middle;",
            ).replace(
                "width: 220px;\n                background: Theme.middle;",
                "width: Theme.middle-width;\n                background: Theme.middle;",
            )
        if source != original:
            open(path, "w", encoding="utf-8", newline="").write(source)
            changed += 1
            print("rewrote", path)
    print("files changed:", changed)


if __name__ == "__main__":
    main()
