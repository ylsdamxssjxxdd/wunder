from __future__ import annotations

import argparse
from pathlib import Path
import sys


DEFAULT_PATHS = [
    "src",
    "frontend/src",
    "tests",
    "docs",
    "web",
    "scripts",
]

TEXT_SUFFIXES = {
    ".rs",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".vue",
    ".json",
    ".md",
    ".html",
    ".css",
    ".svg",
    ".yml",
    ".yaml",
    ".toml",
    ".py",
    ".ps1",
    ".sh",
    ".sql",
    ".txt",
}

TEXT_NAMES = {
    "Dockerfile",
    "AGENTS.md",
}

IGNORED_DIRS = {
    ".git",
    "target",
    "node_modules",
    "dist",
    "coverage",
    ".next",
    ".nuxt",
    "temp_dir",
    "__pycache__",
}

SUSPICIOUS_TOKENS = [
    "\u951f",
    "\ufffd",
    "\u93c2",
    "\u699b",
    "\u7cef",
    "\u9352",
    "\u93b6",
    "\u94da",
    "\u7481",
    "\u951b",
    "\u9286",
    "\u9ad6",
    "\u6d63",
    "\u57b1",
    "\u95c8",
    "\u95be",
    "\u7002",
    "\u61ce",
]

SUSPICIOUS_FRAGMENTS = [
    "\u5bee\u20ac",
    "\u6d93\u5b58",
    "\u93af\u546e",
    "\u699b\u6a3f",
    "\u93c2\u8336",
    "\u7cef\u8354",
    "\u93d5\u6b3d",
    "\u5b87\u69bc",
    "\u5ba0\u6ff0",
    "\u934f\u509c",
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Scan source files for likely mojibake.")
    parser.add_argument("paths", nargs="*", default=DEFAULT_PATHS)
    parser.add_argument(
        "--exclude",
        action="append",
        default=[],
        help="Skip paths containing the given substring. Can be used multiple times.",
    )
    parser.add_argument(
        "--line-limit",
        type=int,
        default=5,
        help="Maximum suspicious lines to print per file.",
    )
    return parser.parse_args()


def safe_print(message: str = "") -> None:
    encoding = sys.stdout.encoding or "utf-8"
    payload = f"{message}\n".encode(encoding, errors="backslashreplace")
    sys.stdout.buffer.write(payload)


def format_hit(value: str) -> str:
    if len(value) == 1:
        return f"U+{ord(value):04X}"
    return "+".join(f"U+{ord(char):04X}" for char in value)


def should_scan_file(path: Path) -> bool:
    return path.name in TEXT_NAMES or path.suffix.lower() in TEXT_SUFFIXES


def is_excluded(path: Path, patterns: list[str]) -> bool:
    normalized = path.as_posix()
    return any(pattern and pattern in normalized for pattern in patterns)


def iter_files(paths: list[str], excludes: list[str]) -> list[Path]:
    files: list[Path] = []
    for raw_path in paths:
        path = Path(raw_path)
        if not path.exists() or is_excluded(path, excludes):
            continue
        if path.is_file():
            if should_scan_file(path):
                files.append(path)
            continue
        for candidate in path.rglob("*"):
            if any(part in IGNORED_DIRS for part in candidate.parts):
                continue
            if candidate.is_file() and should_scan_file(candidate) and not is_excluded(
                candidate, excludes
            ):
                files.append(candidate)
    files.sort()
    return files


def find_suspicious_lines(text: str, line_limit: int) -> list[tuple[int, list[str], str]]:
    findings: list[tuple[int, list[str], str]] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        hits = sorted({token for token in SUSPICIOUS_TOKENS if token in line})
        hits.extend(fragment for fragment in SUSPICIOUS_FRAGMENTS if fragment in line)
        hits = sorted(set(hits))
        if not hits:
            continue
        snippet = " ".join(line.strip().split())
        if len(snippet) > 160:
            snippet = f"{snippet[:157]}..."
        findings.append((line_number, hits, snippet))
        if len(findings) >= line_limit:
            break
    return findings


def scan_file(path: Path, line_limit: int) -> list[str]:
    issues: list[str] = []
    data = path.read_bytes()
    if b"\x00" in data:
        issues.append("contains NUL byte")
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as exc:
        issues.append(f"invalid UTF-8 at byte {exc.start}")
        return issues
    for line_number, hits, snippet in find_suspicious_lines(text, line_limit):
        hit_text = ", ".join(format_hit(token) for token in hits)
        issues.append(f"line {line_number}: suspicious tokens [{hit_text}] :: {snippet}")
    return issues


def main() -> int:
    args = parse_args()
    files = iter_files(args.paths, args.exclude)
    total_findings = 0
    for path in files:
        issues = scan_file(path, args.line_limit)
        if not issues:
            continue
        total_findings += len(issues)
        safe_print(path.as_posix())
        for issue in issues:
            safe_print(f"  - {issue}")
    if total_findings:
        safe_print(f"\nFound {total_findings} suspicious issue(s).")
        return 1
    safe_print("No suspicious mojibake detected.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
