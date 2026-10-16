#!/usr/bin/env python3
"""Audit two local release binaries against the generated shared asset manifest.
Run after rebuilding both artifacts from the same source revision. Paths and
credentials are deliberately omitted from the machine-readable report.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path


def audit(binary, manifest):
    body = binary.read_bytes()
    entries = re.findall(r'\("([^"]+)", include_bytes!\(r#"([^"]+)"#\)\)', manifest.read_text(encoding="utf-8"))
    if not entries:
        raise ValueError("generated asset manifest is empty")
    missing = []
    digest = hashlib.sha256()
    for name, source in entries:
        payload = Path(source).read_bytes()
        digest.update(name.encode()); digest.update(b"\0"); digest.update(payload)
        if payload not in body:
            missing.append(name)
    return {"bytes": len(body), "sha256": hashlib.sha256(body).hexdigest(),
            "assets": len(entries), "asset_digest": digest.hexdigest(), "missing": missing}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--cli-manifest", type=Path, required=True)
    parser.add_argument("--desktop-manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    cli = audit(args.cli, args.cli_manifest)
    desktop = audit(args.desktop, args.desktop_manifest)
    passed = not cli["missing"] and not desktop["missing"] and cli["asset_digest"] == desktop["asset_digest"]
    report = {"passed": passed, "cli": cli, "desktop": desktop,
              "target_os_smoke": "required separately", "startup_latency": "not measured"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({"passed": passed, "cli_assets": cli["assets"], "desktop_assets": desktop["assets"]}))
    raise SystemExit(0 if passed else 1)


if __name__ == "__main__":
    main()
