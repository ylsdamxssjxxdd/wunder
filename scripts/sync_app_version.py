#!/usr/bin/env python
# -*- coding: utf-8 -*-

from __future__ import annotations

import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
APP_VERSION_PATH = ROOT / "config" / "app_version.json"
SEMVER_PATTERN = re.compile(r"^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$")


def load_app_version() -> str:
    payload = json.loads(APP_VERSION_PATH.read_text(encoding="utf-8"))
    version = str(payload.get("version", "")).strip()
    if not version:
        raise ValueError(f"missing version in {APP_VERSION_PATH}")
    if not SEMVER_PATTERN.fullmatch(version):
        raise ValueError(f"invalid semver version: {version}")
    return version


def write_json(path: Path, updater) -> bool:
    payload = json.loads(path.read_text(encoding="utf-8"))
    changed = updater(payload)
    if not changed:
        return False
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return True


def set_version_field(payload: dict, version: str) -> bool:
    current = str(payload.get("version", "")).strip()
    if current == version:
        return False
    payload["version"] = version
    return True


def replace_package_version(path: Path, version: str) -> bool:
    text = path.read_text(encoding="utf-8-sig")
    pattern = re.compile(
        r'(?ms)(^\[package\]\s.*?^version\s*=\s*")[^"]+("\s*$)'
    )
    updated, count = pattern.subn(rf"\g<1>{version}\2", text, count=1)
    if count == 0 or updated == text:
        return False
    path.write_text(updated, encoding="utf-8")
    return True


def main() -> int:
    version = load_app_version()
    changed_files: list[str] = []

    # These manifest files still require static versions for packaging/update metadata,
    # so we sync them from the single source of truth in config/app_version.json.
    sync_targets = [
        ROOT / "Cargo.toml",
        ROOT / "desktop" / "tauri" / "Cargo.toml",
    ]
    for target in sync_targets:
        if replace_package_version(target, version):
            changed_files.append(str(target.relative_to(ROOT)))

    json_targets = [
        ROOT / "frontend" / "package.json",
        ROOT / "desktop" / "electron" / "package.json",
        ROOT / "desktop" / "tauri" / "tauri.conf.json",
    ]
    for target in json_targets:
        if write_json(target, lambda payload, value=version: set_version_field(payload, value)):
            changed_files.append(str(target.relative_to(ROOT)))

    if changed_files:
        print(f"Synced app version {version}:")
        for item in changed_files:
            print(f"- {item}")
    else:
        print(f"App version already synced: {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
