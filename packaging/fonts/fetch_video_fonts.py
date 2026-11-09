#!/usr/bin/env python3
"""Fetch the 8 OFL fonts used by the 视频制作 skill into a supplement build directory.

Fonts never enter git. Build scripts call this with either a local source dir
(config/fonts/video on a deployment host) or the pinned upstream URLs in
video-fonts.json. Every file is verified by SHA-256 before it is used.
"""
import argparse, hashlib, json, shutil, sys, time, urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
DEFAULT_MANIFEST = HERE / "video-fonts.json"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def download(url, target, timeout, retries):
    tmp = target.with_name(target.name + ".tmp")
    last = None
    for attempt in range(1, retries + 1):
        print("download", target.name, "<-", url, f"({attempt}/{retries})", flush=True)
        try:
            with urllib.request.urlopen(url, timeout=timeout) as r, open(tmp, "wb") as f:
                shutil.copyfileobj(r, f)
            return tmp
        except Exception as e:
            last = e
            try:
                tmp.unlink()
            except FileNotFoundError:
                pass
            if attempt < retries:
                print("  retry after", e, file=sys.stderr, flush=True)
                time.sleep(3)
    raise RuntimeError(f"download failed: {url}: {last}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dest", required=True, help="directory to receive the 8 ttf files")
    ap.add_argument("--source-dir", default="", help="local font directory; takes priority over downloads")
    ap.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    ap.add_argument("--licenses", default="", help="directory holding OFL-*.txt (default: manifest licenseDir)")
    ap.add_argument("--timeout", type=int, default=600, help="per-download socket timeout in seconds (default 600)")
    ap.add_argument("--retries", type=int, default=3, help="download attempts per file (default 3)")
    args = ap.parse_args()

    dest = Path(args.dest).resolve()
    dest.mkdir(parents=True, exist_ok=True)
    spec = json.load(open(args.manifest, encoding="utf-8"))
    source = Path(args.source_dir).resolve() if args.source_dir else None

    for item in spec["files"]:
        name, expected = item["name"], item["sha256"]
        target = dest / name
        if target.is_file() and sha256(target) == expected:
            print("reuse", name, flush=True)
            continue
        src = source / name if source else None
        if src and src.is_file():
            shutil.copyfile(src, target)
            print("copy", name, "<-", src, flush=True)
        else:
            try:
                tmp = download(item["url"], target, args.timeout, args.retries)
            except RuntimeError as e:
                sys.exit(str(e))
            tmp.replace(target)
        actual = sha256(target)
        if actual != expected:
            try:
                target.unlink()
            except FileNotFoundError:
                pass
            sys.exit(f"font hash mismatch: {name}: {actual} != {expected}")

    license_dir = Path(args.licenses).resolve() if args.licenses else (REPO / spec["licenseDir"])
    licenses = sorted(license_dir.glob("OFL-*.txt")) if license_dir.is_dir() else []
    if not licenses:
        print("warning: no OFL-*.txt found in", license_dir, file=sys.stderr)
    for p in licenses:
        shutil.copyfile(p, dest / p.name)
    print(f"video fonts: {len(spec['files'])} ttf -> {dest}", flush=True)


if __name__ == "__main__":
    main()
