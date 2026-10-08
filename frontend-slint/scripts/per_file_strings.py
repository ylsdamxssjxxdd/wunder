"""One-off audit: per-file unique Chinese string literals in ui/*.slint."""
import collections
import glob
import re

PATTERN = re.compile(r'"((?:[^"\\]|\\.)*)"')


def has_cjk(text):
    return any("\u4e00" <= ch <= "\u9fff" for ch in text)


def main():
    out = []
    for path in sorted(glob.glob("ui/*.slint")):
        src = re.sub(r"//[^\n]*", "", open(path, encoding="utf-8").read())
        seen = collections.OrderedDict()
        for m in PATTERN.finditer(src):
            s = m.group(1)
            if has_cjk(s):
                seen[s] = 1
        if seen:
            out.append(f"===== {path} ({len(seen)}) =====")
            out.extend(seen)
    open("zh-per-file.txt", "w", encoding="utf-8").write("\n".join(out))
    print("written", len(out), "lines")


if __name__ == "__main__":
    main()
