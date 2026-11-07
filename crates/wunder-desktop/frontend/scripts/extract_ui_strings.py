"""One-off audit: list Chinese string literals in ui/*.slint with context."""
import collections
import glob
import json
import re
import sys

PATTERN = re.compile(r'"((?:[^"\\]|\\.)*)"')


def has_cjk(text):
    return any("\u4e00" <= ch <= "\u9fff" for ch in text)


def main():
    total = 0
    unique = collections.Counter()
    interpolated = []
    per_file = {}
    for path in sorted(glob.glob("ui/*.slint")):
        source = open(path, encoding="utf-8").read()
        count = 0
        cleaned = re.sub(r"//[^\n]*", "", source)
        for match in PATTERN.finditer(cleaned):
            text = match.group(1)
            if has_cjk(text):
                total += 1
                count += 1
                unique[text] += 1
                if "\\{" in text:
                    interpolated.append((path, text))
        per_file[path] = count
    print("total instances:", total)
    print("unique strings:", len(unique))
    for path, count in per_file.items():
        print(f"{path}: {count}")
    print("interpolated strings:", len(interpolated))
    for path, text in interpolated:
        print("  ", path, repr(text))
    with open(sys.argv[1] if len(sys.argv) > 1 else "ui-strings.json", "w", encoding="utf-8") as fh:
        json.dump(
            [{"zh": text, "count": count} for text, count in unique.most_common()],
            fh,
            ensure_ascii=False,
            indent=1,
        )


if __name__ == "__main__":
    main()
