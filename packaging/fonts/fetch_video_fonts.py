#!/usr/bin/env python3
"""Fetch font components used by the bundled skills into a deployment/download directory.

Fonts never enter git. Build scripts and scripts/fetch_fonts.py call this with either
a local source dir (config/fonts/<set>) or the pinned upstream URLs in
video-fonts.json / common-fonts.json. Every file is verified by SHA-256 before use;
zip-only releases may pin an entry and the archive hash.
"""
import argparse, hashlib, json, shutil, sys, time, urllib.request, zipfile
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


def item_urls(item):
    urls = item.get("urls") or ([item["url"]] if item.get("url") else [])
    if not urls:
        raise SystemExit(f"font entry has no url: {item.get('name', '?')}")
    return urls


def download(urls, target, timeout, retries):
    tmp = target.with_name(target.name + ".tmp")
    errors = []
    for url in urls:
        for attempt in range(1, retries + 1):
            print("download", target.name, "<-", url, f"({attempt}/{retries})", flush=True)
            try:
                with urllib.request.urlopen(url, timeout=timeout) as r, open(tmp, "wb") as f:
                    shutil.copyfileobj(r, f)
                return tmp, url
            except Exception as e:
                errors.append(f"{url}: {e}")
                try:
                    tmp.unlink()
                except FileNotFoundError:
                    pass
                if attempt < retries:
                    print("  retry after", e, file=sys.stderr, flush=True)
                    time.sleep(3)
    raise RuntimeError("download failed: " + "; ".join(errors))


def find_source(source, name):
    exact = source / name
    if exact.is_file():
        return exact
    lowered = name.lower()
    for candidate in source.iterdir():
        if candidate.name.lower() == lowered and candidate.is_file():
            return candidate
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dest", required=True, help="directory to receive the font files")
    ap.add_argument("--source-dir", default="", help="local font directory; takes priority over downloads")
    ap.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    ap.add_argument("--licenses", default="", help="directory holding license texts (default: manifest licenseDir)")
    ap.add_argument("--timeout", type=int, default=600, help="per-download socket timeout in seconds (default 600)")
    ap.add_argument("--retries", type=int, default=3, help="download attempts per URL (default 3)")
    ap.add_argument("--check", action="store_true", help="only verify files already present in --dest")
    args = ap.parse_args()

    dest = Path(args.dest).resolve()
    spec = json.load(open(args.manifest, encoding="utf-8"))
    source = Path(args.source_dir).resolve() if args.source_dir else None
    if source and not source.is_dir():
        sys.exit(f"font source dir not found: {source}")

    if args.check:
        bad = []
        for item in spec["files"]:
            target = dest / item["name"]
            if not target.is_file():
                print("missing", item["name"], flush=True)
                bad.append(item["name"])
                continue
            actual = sha256(target)
            if actual != item["sha256"]:
                print("mismatch", item["name"], actual, "!=", item["sha256"], flush=True)
                bad.append(item["name"])
        if bad:
            print(f"{len(bad)}/{len(spec['files'])} font files missing or corrupt in {dest}", file=sys.stderr)
            sys.exit(1)
        print(f"fonts ok: {len(spec['files'])} files in {dest}", flush=True)
        return

    dest.mkdir(parents=True, exist_ok=True)
    for item in spec["files"]:
        name, expected = item["name"], item["sha256"]
        target = dest / name
        if target.is_file() and sha256(target) == expected:
            print("reuse", name, flush=True)
            continue
        src = find_source(source, name) if source else None
        if src:
            shutil.copyfile(src, target)
            print("copy", name, "<-", src, flush=True)
        else:
            try:
                tmp, _ = download(item_urls(item), target, args.timeout, args.retries)
            except RuntimeError as e:
                sys.exit(str(e))
            if item.get("archive") == "zip":
                try:
                    archive_actual = sha256(tmp)
                    archive_expected = item.get("zipSha256", "")
                    if archive_expected and archive_actual != archive_expected:
                        sys.exit(f"font archive hash mismatch: {item['name']}: {archive_actual} != {archive_expected}")
                    with zipfile.ZipFile(str(tmp)) as archive:
                        target.write_bytes(archive.read(item["entry"]))
                finally:
                    try:
                        tmp.unlink()
                    except FileNotFoundError:
                        pass
            else:
                tmp.replace(target)
        actual = sha256(target)
        if actual != expected:
            try:
                target.unlink()
            except FileNotFoundError:
                pass
            sys.exit(f"font hash mismatch: {name}: {actual} != {expected}")

    license_dir = Path(args.licenses).resolve() if args.licenses else (
        REPO / spec["licenseDir"] if spec.get("licenseDir") else None)
    licenses = sorted(license_dir.glob("OFL-*.txt")) if license_dir and license_dir.is_dir() else []
    for p in licenses:
        shutil.copyfile(p, dest / p.name)
    print(f"fonts: {len(spec['files'])} files -> {dest}", flush=True)


if __name__ == "__main__":
    main()
