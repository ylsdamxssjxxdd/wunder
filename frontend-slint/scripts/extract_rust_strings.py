"""One-off audit: distinct Rust UI-facing strings (set_status/dialog/titles)."""
import collections
import glob
import re

PATTERNS = [
    ("s", re.compile(r'set_status\(\s*"((?:[^"\\]|\\.)*)"')),
    ("f", re.compile(r'set_status\(\s*format!\(\s*"((?:[^"\\]|\\.)*)"')),
    ("d", re.compile(r'set_dialog_(?:title|text)\(\s*(?:format!\()?\s*"((?:[^"\\]|\\.)*)"')),
    ("t", re.compile(r'set_(?:world_active_title|thread_log_title|cron_runs_title|heading)\(\s*[^;]*?"((?:[^"\\]|\\.)*)"')),
]


def has_cjk(text):
    return any("\u4e00" <= ch <= "\u9fff" for ch in text)


def main():
    uniq = collections.Counter()
    for path in glob.glob("src/*.rs"):
        src = open(path, encoding="utf-8").read()
        for tag, pat in PATTERNS:
            for m in pat.finditer(src):
                s = m.group(1)
                if has_cjk(s):
                    uniq[(tag, s)] += 1
    for (tag, s), _n in sorted(uniq.items(), key=lambda kv: kv[0][1]):
        print(f"{tag}|{s}")


if __name__ == "__main__":
    main()
