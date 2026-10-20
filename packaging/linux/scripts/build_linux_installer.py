#!/usr/bin/env python3
"""Assemble the Wunder Linux self-extracting installer.

Inputs are the release AppImage, the CLI ELF, and the Linux supplement tar.gz.
The supplement is normalized to opt/<component>/... while preserving modes and
relative symlinks; its original ./opt/... layout is accepted too. The result is
one executable .run file: a POSIX shell header followed by a gzip-compressed
payload tar.

This script stays compatible with Python 3.6 (Ubuntu 18.04 build hosts).
"""

import argparse
import gzip
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tarfile
import time
from pathlib import Path

KNOWN_COMPONENTS = ("python", "git", "rg", "ffmpeg", "browser")
ELF_MACHINES = {"amd64": 0x3E, "arm64": 0xB7}
LABELS = {
    "python": "Python {version} + 常用库",
    "git": "Git {version}",
    "rg": "ripgrep {version}",
    "ffmpeg": "FFmpeg {version}",
    "browser": "内置浏览器 Chrome {version}",
}
CHUNK = 4 * 1024 * 1024


def open_compressed_tar(path):
    # tarfile.open(..., "w|gz", compresslevel=...) needs Python 3.9, but the
    # offline packer may run on Python 3.6/3.8 (Ubuntu 18.04, embedded Win).
    raw = path.open("wb")
    try:
        gz = gzip.GzipFile(fileobj=raw, mode="wb", compresslevel=1, mtime=0)
        try:
            tar = tarfile.open(fileobj=gz, mode="w|", format=tarfile.GNU_FORMAT)
        except Exception:
            gz.close()
            raise
    except Exception:
        raw.close()
        raise
    return tar, gz, raw


def close_compressed_tar(tar, gz, raw):
    try:
        tar.close()
    finally:
        try:
            gz.close()
        finally:
            raw.close()


def fail(message):
    print("[wunder-pack] " + message, file=sys.stderr)
    raise SystemExit(2)


def repo_root():
    return Path(__file__).resolve().parents[3]


def sha256_file(path):
    digest = hashlib.sha256()
    handle = path.open("rb")
    try:
        while True:
            block = handle.read(CHUNK)
            if not block:
                break
            digest.update(block)
    finally:
        handle.close()
    return digest.hexdigest()


def contains_bytes(path, needle):
    overlap = len(needle) - 1
    tail = b""
    handle = path.open("rb")
    try:
        while True:
            block = handle.read(CHUNK)
            if not block:
                return False
            combined = tail + block
            if needle in combined:
                return True
            tail = combined[-overlap:] if overlap else b""
    finally:
        handle.close()


def read_version(root):
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not match:
        fail("could not read the workspace version from " + str(root / "Cargo.toml"))
    return match.group(1)


def read_frontend_version(root):
    text = (root / "frontend-slint" / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not match:
        fail("could not read the frontend-slint version")
    return match.group(1)


def resolve_path(root, value):
    if not value:
        return None
    candidate = Path(value)
    if not candidate.is_absolute():
        candidate = root / candidate
    return candidate.resolve()


def find_supplement(root, arch):
    patterns = (
        root / "dist" / ("linux-" + arch) / ("wunder-supplement-linux-" + arch + ".tar.gz"),
        root / "target" / "installer" / "payload" / ("linux-" + arch) / ("wunder-supplement-linux-" + arch + ".tar.gz"),
        root / "temp_dir" / "linux-supplement" / "dist" / ("wunder-supplement-linux-" + arch + ".tar.gz"),
    )
    found = [path for path in patterns if path.is_file()]
    if not found:
        return None
    if len(found) > 1:
        listing = "\n".join(str(path) for path in found)
        fail("multiple supplement archives found; pass --supplement explicitly:\n" + listing)
    return found[0].resolve()


def validate_elf(path, arch, appimage):
    if not path.is_file():
        fail("input is missing: " + str(path))
    handle = path.open("rb")
    try:
        header = handle.read(64)
    finally:
        handle.close()
    if len(header) < 20 or header[:4] != b"\x7fELF":
        fail("not an ELF file: " + str(path))
    if header[5] != 1:
        fail("ELF file is not little-endian: " + str(path))
    machine = int.from_bytes(header[18:20], "little")
    if machine != ELF_MACHINES[arch]:
        fail(
            "ELF architecture mismatch: expected %s (0x%x), got 0x%x: %s"
            % (arch, ELF_MACHINES[arch], machine, path)
        )
    if appimage and not contains_bytes(path, b"hsqs"):
        fail("desktop artifact is not an AppImage with a type-2 squashfs payload: " + str(path))


def safe_member_name(raw):
    name = raw.replace("\\", "/")
    while name.startswith("./"):
        name = name[2:]
    if not name or name.startswith("/"):
        return None
    parts = [part for part in name.split("/") if part not in ("", ".")]
    if not parts or ".." in parts:
        return None
    return "/".join(parts)


def safe_link_name(raw):
    if not raw:
        return None
    name = raw.replace("\\", "/")
    if name.startswith("/"):
        return None
    parts = [part for part in name.split("/") if part not in ("", ".")]
    if not parts or ".." in parts:
        return None
    return "/".join(parts)


def normalize_supplement(source, destination):
    components = dict((key, {"bytes": 0, "files": 0, "version": ""}) for key in KNOWN_COMPONENTS)
    metadata = {}
    temporary = destination.with_name(destination.name + ".tmp")
    if temporary.exists():
        temporary.unlink()

    try:
        source_tar = tarfile.open(str(source), "r|gz")
    except (tarfile.TarError, OSError) as error:
        fail("cannot read supplement archive %s: %s" % (source, error))

    try:
        target_tar, target_gzip, target_raw = open_compressed_tar(temporary)
        try:
            for member in source_tar:
                raw_name = member.name.replace("\\", "/")
                while raw_name.startswith("./"):
                    raw_name = raw_name[2:]
                if raw_name in ("", ".", "/"):
                    continue
                name = safe_member_name(member.name)
                if name is None:
                    fail("supplement contains an unsafe path: " + member.name)
                if name == "wunder-linux-supplement.json":
                    extracted = source_tar.extractfile(member)
                    if extracted is not None:
                        try:
                            metadata = json.loads(extracted.read().decode("utf-8-sig"))
                        except (UnicodeDecodeError, ValueError):
                            metadata = {}
                    continue
                parts = name.split("/")
                if len(parts) < 2 or parts[0] != "opt" or parts[1] not in components:
                    continue
                component = parts[1]
                if member.isdir():
                    pass
                elif member.isfile():
                    components[component]["bytes"] += member.size
                    components[component]["files"] += 1
                elif member.issym():
                    link = safe_link_name(member.linkname)
                    if link is None:
                        fail(
                            "supplement contains an unsafe symlink: %s -> %s"
                            % (member.name, member.linkname)
                        )
                    member.linkname = link
                    components[component]["files"] += 1
                elif member.islnk():
                    link = safe_link_name(member.linkname)
                    link_parts = link.split("/") if link else []
                    if (
                        len(link_parts) < 2
                        or link_parts[0] != "opt"
                        or link_parts[1] != component
                    ):
                        fail(
                            "supplement contains a hardlink that crosses components: %s -> %s"
                            % (member.name, member.linkname)
                        )
                    member.linkname = link
                    components[component]["files"] += 1
                else:
                    continue

                copied = tarfile.TarInfo(name)
                copied.mode = member.mode
                copied.mtime = member.mtime
                copied.uid = 0
                copied.gid = 0
                copied.uname = ""
                copied.gname = ""
                copied.type = member.type
                copied.linkname = member.linkname if (member.issym() or member.islnk()) else ""
                if member.isdir():
                    target_tar.addfile(copied)
                elif member.isfile():
                    extracted = source_tar.extractfile(member)
                    if extracted is None:
                        fail("cannot read supplement member: " + member.name)
                    copied.size = member.size
                    target_tar.addfile(copied, extracted)
                elif member.issym() or member.islnk():
                    target_tar.addfile(copied)
        finally:
            close_compressed_tar(target_tar, target_gzip, target_raw)
    finally:
        source_tar.close()

    present = [key for key in KNOWN_COMPONENTS if components[key]["files"] > 0]
    if not present:
        if temporary.exists():
            temporary.unlink()
        fail("supplement archive has no opt/<component> payload: " + str(source))

    for key in present:
        entry = metadata.get(key) if isinstance(metadata, dict) else None
        if isinstance(entry, dict) and isinstance(entry.get("version"), str):
            components[key]["version"] = entry["version"]

    os.replace(str(temporary), str(destination))
    return dict((key, components[key]) for key in present)


def component_label(key, info):
    label = LABELS.get(key, key).format(version=info.get("version", ""))
    return " ".join(label.split())


def add_bytes(archive, arcname, data, mode):
    info = tarfile.TarInfo(arcname)
    info.size = len(data)
    info.mode = mode
    info.mtime = int(time.time())
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    archive.addfile(info, io.BytesIO(data))


def add_file(archive, arcname, source, mode):
    stat_result = source.stat()
    info = tarfile.TarInfo(arcname)
    info.size = stat_result.st_size
    info.mode = mode
    info.mtime = int(stat_result.st_mtime)
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    handle = source.open("rb")
    try:
        archive.addfile(info, handle)
    finally:
        handle.close()


def build_payload(stage, version, arch, desktop, cli, core_only, components, supplement, installer_dir, image):
    payload = stage / "payload.tar.gz"
    temporary = stage / "payload.tar.gz.tmp"
    if temporary.exists():
        temporary.unlink()

    manifest = {
        "product": "wunder",
        "version": version,
        "target": "linux-" + arch,
        "coreOnly": core_only,
        "builtAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "components": [
            {
                "name": key,
                "label": component_label(key, info),
                "path": "opt/" + key,
                "version": info.get("version", ""),
                "bytes": info["bytes"],
                "files": info["files"],
                "defaultSelected": False,
            }
            for key, info in components.items()
        ],
    }
    components_text = "".join(
        "%s|%s|opt/%s|%s|%s|%s\n"
        % (
            key,
            component_label(key, info),
            key,
            info.get("version", ""),
            info["bytes"],
            info["files"],
        )
        for key, info in components.items()
    )

    target_tar, target_gzip, target_raw = open_compressed_tar(temporary)
    try:
        directory = tarfile.TarInfo("payload")
        directory.type = tarfile.DIRTYPE
        directory.mode = 0o755
        directory.mtime = int(time.time())
        directory.uid = 0
        directory.gid = 0
        directory.uname = ""
        directory.gname = ""
        target_tar.addfile(directory)

        add_bytes(target_tar, "payload/version", (version + "\n").encode("utf-8"), 0o644)
        add_bytes(target_tar, "payload/arch", (arch + "\n").encode("utf-8"), 0o644)
        add_bytes(target_tar, "payload/components.txt", components_text.encode("utf-8"), 0o644)
        add_bytes(
            target_tar,
            "payload/manifest.json",
            (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8"),
            0o644,
        )
        add_file(target_tar, "payload/wunder-desktop.AppImage", desktop, 0o755)
        add_file(target_tar, "payload/wunder-cli", cli, 0o755)
        if image is not None and image.is_file():
            add_file(target_tar, "payload/wunder.png", image, 0o644)
        add_file(target_tar, "payload/install.sh", installer_dir / "install.sh", 0o755)
        add_file(target_tar, "payload/uninstall.sh", installer_dir / "uninstall.sh", 0o755)
        if not core_only:
            if supplement is None:
                fail("internal error: supplement path is missing")
            add_file(target_tar, "payload/supplement.tar.gz", supplement, 0o644)
    finally:
        close_compressed_tar(target_tar, target_gzip, target_raw)

    os.replace(str(temporary), str(payload))
    return payload


def build_run(stage, output, installer_dir, payload):
    header = (installer_dir / "run-header.sh").read_bytes().replace(b"\r\n", b"\n")
    marker = b"__WUNDER_PAYLOAD_BELOW__"
    if marker not in header:
        fail("run-header.sh does not contain the payload marker")
    if not header.endswith(b"\n"):
        header += b"\n"

    temporary = output.with_name(output.name + ".tmp")
    if temporary.exists():
        temporary.unlink()
    output_handle = temporary.open("wb")
    try:
        output_handle.write(header)
        source = payload.open("rb")
        try:
            shutil.copyfileobj(source, output_handle, CHUNK)
        finally:
            source.close()
    finally:
        output_handle.close()
    if os.name != "nt":
        temporary.chmod(0o755)
    os.replace(str(temporary), str(output))
    if os.name != "nt":
        output.chmod(0o755)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arch", choices=sorted(ELF_MACHINES), required=True)
    parser.add_argument("--version", default="")
    parser.add_argument("--desktop", default="")
    parser.add_argument("--cli", default="")
    parser.add_argument("--supplement", default="")
    parser.add_argument("--output-dir", default="")
    parser.add_argument("--core-only", action="store_true")
    args = parser.parse_args()

    root = repo_root()
    version = args.version or read_version(root)
    if not args.version:
        frontend_version = read_frontend_version(root)
        if frontend_version != version:
            fail("version mismatch: workspace=%s frontend-slint=%s" % (version, frontend_version))

    desktop = resolve_path(root, args.desktop) or (
        root
        / "target"
        / "slint"
        / "dist"
        / ("linux-" + args.arch)
        / ("wunder-slint-%s-linux-%s.AppImage" % (version, args.arch))
    )
    cli = resolve_path(root, args.cli) or (
        root
        / "target"
        / "cli"
        / "dist"
        / ("linux-" + args.arch)
        / ("wunder-cli-%s-linux-%s" % (version, args.arch))
    )
    supplement = resolve_path(root, args.supplement) if args.supplement else (
        None if args.core_only else find_supplement(root, args.arch)
    )

    validate_elf(desktop, args.arch, appimage=True)
    validate_elf(cli, args.arch, appimage=False)
    if not args.core_only:
        if supplement is None:
            fail(
                "Linux supplement archive was not found. Build it or pass --supplement, "
                "or use --core-only for a core-only installer."
            )
        if not supplement.is_file() or not str(supplement).endswith((".tar.gz", ".tgz")):
            fail("supplement must be a .tar.gz archive: " + str(supplement))

    output_dir = resolve_path(root, args.output_dir) or (
        root / "target" / "installer" / "dist" / ("linux-" + args.arch)
    )
    stage = root / "temp_dir" / "installer" / ("linux-" + args.arch)
    output_dir.mkdir(parents=True, exist_ok=True)
    stage.mkdir(parents=True, exist_ok=True)

    installer_dir = root / "packaging" / "linux" / "installer"
    image = root / "images" / "wunder.png"
    components = {}
    normalized_supplement = None

    if not args.core_only:
        source_hash = sha256_file(supplement)
        normalized_supplement = stage / "supplement.tar.gz"
        cache_path = stage / "supplement.meta.json"
        cached = None
        if cache_path.is_file() and normalized_supplement.is_file():
            try:
                cached = json.loads(cache_path.read_text(encoding="utf-8"))
            except (UnicodeDecodeError, ValueError):
                cached = None
        if (
            cached
            and cached.get("sourceSha256") == source_hash
            and cached.get("archiveSha256") == sha256_file(normalized_supplement)
            and cached.get("components")
        ):
            components = cached["components"]
            print("[wunder-pack] reusing normalized supplement stage")
        else:
            print("[wunder-pack] normalizing supplement archive")
            components = normalize_supplement(supplement, normalized_supplement)
            cache = {
                "sourceSha256": source_hash,
                "archiveSha256": sha256_file(normalized_supplement),
                "components": components,
            }
            cache_path.write_text(json.dumps(cache, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        if not components:
            fail("supplement archive contains no recognized components")

    print("[wunder-pack] assembling payload")
    payload = build_payload(
        stage,
        version,
        args.arch,
        desktop,
        cli,
        args.core_only,
        components,
        normalized_supplement,
        installer_dir,
        image,
    )

    output_name = "wunder-%s-linux-%s.run" % (version, args.arch)
    output = output_dir / output_name
    build_run(stage, output, installer_dir, payload)
    installer_hash = sha256_file(output)
    installer_size = output.stat().st_size

    manifest = {
        "product": "wunder",
        "version": version,
        "target": "linux-" + args.arch,
        "installer": {
            "file": output.name,
            "bytes": installer_size,
            "sha256": installer_hash,
        },
        "coreOnly": args.core_only,
        "builtAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "components": [
            {
                "name": key,
                "label": component_label(key, info),
                "version": info.get("version", ""),
                "bytes": info["bytes"],
                "files": info["files"],
                "defaultSelected": False,
            }
            for key, info in components.items()
        ],
        "inputs": [
            {
                "role": "desktop",
                "file": desktop.name,
                "bytes": desktop.stat().st_size,
                "sha256": sha256_file(desktop),
            },
            {
                "role": "cli",
                "file": cli.name,
                "bytes": cli.stat().st_size,
                "sha256": sha256_file(cli),
            },
        ],
    }
    if supplement is not None:
        manifest["inputs"].append(
            {
                "role": "supplement",
                "file": supplement.name,
                "bytes": supplement.stat().st_size,
                "sha256": sha256_file(supplement),
            }
        )

    manifest_path = output_dir / (output_name + ".manifest.json")
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    (output_dir / (output_name + ".sha256")).write_text(
        installer_hash + "  " + output.name + "\n", encoding="utf-8"
    )

    print("[wunder-pack] installer: " + str(output))
    print("[wunder-pack] size: %.1f MB" % (installer_size / (1024.0 * 1024.0)))
    print("[wunder-pack] sha256: " + installer_hash)
    print("[wunder-pack] manifest: " + str(manifest_path))
    if os.name == "nt":
        print("[wunder-pack] the .run file cannot carry a Unix execute bit on Windows; run chmod +x on Linux")


if __name__ == "__main__":
    main()
