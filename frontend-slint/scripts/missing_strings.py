# -*- coding: utf-8 -*-
"""List CJK literals in a slint file that are missing from the catalog."""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from build_i18n import SKIP_FILES, load_catalog  # noqa: E402
from slint_scan import has_cjk, scan  # noqa: E402

ROOT = os.path.dirname(HERE)


def main():
    entries = load_catalog()
    zh_set = {entry[1] for entry in entries}
    for path in sorted(glob_or_args(sys.argv[1:])):
        base = os.path.basename(path)
        if base in SKIP_FILES:
            continue
        src = open(path, encoding="utf-8").read()
        seen = []
        for _start, _end, s in scan(src):
            if has_cjk(s) and s not in zh_set and s not in seen:
                seen.append(s)
        if seen:
            print(f"== {base} ==")
            for s in seen:
                print(f"  {s!r}")


def glob_or_args(args):
    if args:
        return args
    return [os.path.join(ROOT, "ui", f) for f in sorted(os.listdir(os.path.join(ROOT, "ui"))) if f.endswith(".slint")]


if __name__ == "__main__":
    main()
