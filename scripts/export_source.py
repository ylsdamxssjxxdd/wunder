#!/usr/bin/env python
"""Export the current wunder source tree to a validated ZIP archive.

Selection follows the same model as rcho's export_source.py: every file git
reports (tracked, or untracked but not ignored) is exported, minus build
output and temp directories. The archive must be buildable on another machine,
so committed third-party trees required by the build (frontend-slint/vendor
with the patched Slint 1.18 sources, web/third for the admin console) are
included; web/docs is excluded because it is the built help site and is
regenerated with `python scripts/build_docs_site.py`.
"""

from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
import zipfile


REPO_ROOT = Path(__file__).resolve().parent.parent
EXCLUDED_PREFIXES = (
    PurePosixPath(".git"),
    PurePosixPath("target"),
    PurePosixPath("temp_dir"),
    PurePosixPath("frontend/dist"),
    PurePosixPath("web/docs"),
)
EXCLUDED_PARTS = {"__pycache__", ".pytest_cache", "node_modules", ".git"}

# Workspace anchors: if any of these is missing the worktree state is broken
# and the archive would mislead whoever receives it. The vendored Slint core
# anchor guarantees the desktop frontend can be rebuilt from the archive.
REQUIRED_FILES = (
    PurePosixPath("Cargo.toml"),
    PurePosixPath("crates/wunder-runtime/Cargo.toml"),
    PurePosixPath("crates/wunder-server/Cargo.toml"),
    PurePosixPath("crates/wunder-cli/Cargo.toml"),
    PurePosixPath("crates/wunder-desktop/Cargo.toml"),
    PurePosixPath("frontend-slint/Cargo.toml"),
    PurePosixPath("frontend-slint/vendor/i-slint-core-1.18/Cargo.toml"),
    PurePosixPath("frontend/package.json"),
    PurePosixPath("web/index.html"),
)

if os.name == "nt":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Export the current wunder source tree to a ZIP archive.",
    )
    parser.add_argument(
        "--output",
        default="",
        help="Output ZIP path or directory. Defaults to the current user's Desktop.",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Overwrite an existing archive at the selected path.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the selected files without creating an archive.",
    )
    return parser.parse_args()


def run_git(*args: str) -> bytes:
    process = subprocess.run(
        ["git", *args],
        cwd=REPO_ROOT,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if process.returncode != 0:
        detail = process.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"git {' '.join(args)} failed: {detail}")
    return process.stdout


def git_worktree_files() -> set[PurePosixPath]:
    output = run_git("-c", "core.quotepath=false", "ls-files", "-z", "--cached", "--others", "--exclude-standard")
    files: set[PurePosixPath] = set()
    for raw in output.split(b"\0"):
        if not raw:
            continue
        relative = PurePosixPath(raw.decode("utf-8", errors="surrogateescape"))
        if not is_excluded(relative):
            files.add(relative)
    return files


def is_excluded(relative: PurePosixPath) -> bool:
    if any(part in EXCLUDED_PARTS for part in relative.parts):
        return True
    return any(relative == prefix or prefix in relative.parents for prefix in EXCLUDED_PREFIXES)


def desktop_dir() -> Path:
    home = Path(os.environ.get("USERPROFILE", "")) if os.name == "nt" else Path.home()
    desktop = home / "Desktop"
    return desktop if desktop.is_dir() else Path.home()


def resolve_output(raw_output: str) -> Path:
    default_name = f"wunder-source-{datetime.now():%Y%m%d-%H%M%S}.zip"
    if not raw_output:
        return desktop_dir() / default_name
    output = Path(raw_output).expanduser()
    if not output.is_absolute():
        output = REPO_ROOT / output
    if output.suffix.lower() != ".zip":
        output = output / default_name
    return output.resolve()


def checked_source_path(relative: PurePosixPath) -> Path | None:
    source = REPO_ROOT / Path(*relative.parts)
    if not source.exists():
        return None
    resolved = source.resolve()
    try:
        resolved.relative_to(REPO_ROOT)
    except ValueError as exc:
        raise RuntimeError(f"refusing to export a path outside the repository: {source}") from exc
    if not source.is_file():
        return None
    return source


def collect_files(output: Path) -> list[tuple[PurePosixPath, Path]]:
    relative_files = git_worktree_files()

    selected: list[tuple[PurePosixPath, Path]] = []
    for relative in sorted(relative_files, key=lambda item: item.as_posix()):
        source = checked_source_path(relative)
        if source is None or source.resolve() == output:
            continue
        selected.append((relative, source))
    if not selected:
        raise RuntimeError("no source files were selected for export")
    selected_names = {relative for relative, _ in selected}
    missing = [str(required) for required in REQUIRED_FILES if required not in selected_names]
    if missing:
        raise RuntimeError(f"required workspace anchors missing from export: {missing}")
    return selected


def zip_date_time(source: Path) -> tuple[int, int, int, int, int, int]:
    # Vendored sources may carry pre-1980 mtimes (tar checkouts); the ZIP
    # format cannot represent those, so clamp to the format's epoch floor.
    try:
        mtime = source.stat().st_mtime
    except OSError:
        return (1980, 1, 1, 0, 0, 0)
    if mtime < 315532800:  # 1980-01-01T00:00:00Z
        return (1980, 1, 1, 0, 0, 0)
    local = datetime.fromtimestamp(mtime)
    return (local.year, local.month, local.day, local.hour, local.minute, local.second)


def write_archive(output: Path, files: list[tuple[PurePosixPath, Path]], force: bool) -> None:
    if output.exists() and not force:
        raise RuntimeError(f"output already exists; use --force to replace it: {output}")
    output.parent.mkdir(parents=True, exist_ok=True)

    temp_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            prefix=f".{output.stem}-",
            suffix=".tmp",
            dir=output.parent,
            delete=False,
        ) as temp_file:
            temp_path = Path(temp_file.name)
        with zipfile.ZipFile(temp_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for relative, source in files:
                info = zipfile.ZipInfo(relative.as_posix(), date_time=zip_date_time(source))
                info.compress_type = zipfile.ZIP_DEFLATED
                with source.open("rb") as reader, archive.open(info, "w") as writer:
                    for chunk in iter(lambda: reader.read(1024 * 1024), b""):
                        writer.write(chunk)
        validate_archive(temp_path, files)
        os.replace(temp_path, output)
        temp_path = None
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)


def validate_archive(archive_path: Path, files: list[tuple[PurePosixPath, Path]]) -> None:
    expected = {relative.as_posix() for relative, _source in files}
    with zipfile.ZipFile(archive_path, "r") as archive:
        broken = archive.testzip()
        if broken:
            raise RuntimeError(f"ZIP integrity check failed at entry: {broken}")
        actual = {name for name in archive.namelist() if not name.endswith("/")}
    if actual != expected:
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        raise RuntimeError(f"ZIP entry mismatch; missing={missing}, extra={extra}")
    forbidden = [name for name in actual if is_excluded(PurePosixPath(name))]
    if forbidden:
        raise RuntimeError(f"ZIP contains excluded paths: {forbidden}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest().upper()


def main() -> int:
    args = parse_args()
    try:
        output = resolve_output(args.output)
        files = collect_files(output)
        if args.dry_run:
            for relative, _source in files:
                print(relative.as_posix())
            print(f"selected files: {len(files)}")
            return 0
        write_archive(output, files, args.force)
        print(f"source archive: {output}")
        print(f"files: {len(files)}")
        print(f"bytes: {output.stat().st_size}")
        print(f"sha256: {sha256(output)}")
        return 0
    except (OSError, RuntimeError, ValueError, zipfile.BadZipFile) as exc:
        print(f"source export failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
