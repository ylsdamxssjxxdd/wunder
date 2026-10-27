"""Exercise local Gitea push, Actions matrix, offline toolchains and release publishing."""

import base64
import io
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import time
import zipfile
from urllib.request import Request, urlopen


def main():
    root = Path(__file__).resolve().parents[2]
    compose = ["docker", "compose", "-f", str(root / "packaging/gitea/compose.yaml"),
               "-p", "gitea-offline-validation"]
    temporary_password = secrets.token_urlsafe(32)
    subprocess.run(compose + ["exec", "-T", "--user", "git", "gitea", "gitea", "admin", "user",
                              "change-password", "--username", "admin", "--password", temporary_password,
                              "--must-change-password=false"], check=True, stdout=subprocess.DEVNULL)
    authorization = base64.b64encode(("admin:" + temporary_password).encode()).decode()

    def api(path, body=None):
        request = Request("http://localhost:13000/api/v1" + path,
                          data=None if body is None else json.dumps(body).encode(),
                          headers={"Authorization": "Basic " + authorization,
                                   "Content-Type": "application/json"})
        with urlopen(request, timeout=30) as response:
            return json.load(response)

    name = "offline-check-" + str(int(time.time()))
    api("/user/repos", {"name": name, "private": True, "auto_init": False})
    created = api("/repos/admin/" + name)
    if not created.get("has_actions"):
        raise RuntimeError("Actions is not enabled for newly created repositories")
    directory = root / "target/gitea-validation" / name
    directory.mkdir(parents=True)
    workflow = directory / ".gitea/workflows/probe.yml"
    workflow.parent.mkdir(parents=True)
    workflow.write_text("""name: Offline stack probe
on: [push]
permissions:
  contents: write
jobs:
  probe:
    runs-on: linux-offline-arm64
    strategy:
      fail-fast: false
      max-parallel: 1
      matrix:
        arch: [arm64, amd64]
    steps:
      - name: Local checkout
        env:
          CI_REPOSITORY: ${{ github.repository }}
          CI_COMMIT: ${{ github.sha }}
          CI_REF: ${{ github.ref }}
          CI_TOKEN: ${{ github.token }}
        run: python3 /opt/offline-ci/checkout.py
      - name: Offline isolation probe
        env:
          ARCH: ${{ matrix.arch }}
        run: bash probe.sh
      - name: Original offline release build scripts
        env:
          ARCH: ${{ matrix.arch }}
          CI_REPOSITORY: ${{ github.repository }}
        run: bash packaging/gitea/ci/build.sh
      - name: Native output smoke test
        env:
          ARCH: ${{ matrix.arch }}
        run: |
          if [[ "$ARCH" == arm64 ]]; then
            ./release-assets/wunder-cli-0.0.0-linux-arm64
            tail -c +"$(($(stat -c %s "$WUNDER_OFFLINE_ROOT/appimage-runtime/runtime-aarch64") + 1))" \\
              ./release-assets/wunder-slint-0.0.0-linux-arm64.AppImage > appimage.squashfs
            unsquashfs -processors 2 -d appimage-extracted appimage.squashfs >/dev/null
            ./appimage-extracted/AppRun
          fi
      - name: Local release upload
        env:
          ARCH: ${{ matrix.arch }}
          CI_REPOSITORY: ${{ github.repository }}
          CI_COMMIT: ${{ github.sha }}
          CI_TOKEN: ${{ github.token }}
        run: python3 /opt/offline-ci/publish.py release-assets
""", encoding="utf-8")
    (directory / "probe.sh").write_text("""#!/bin/bash
set -euo pipefail
test ! -S /var/run/docker.sock
test ! -e /bootstrap
test -x "$WUNDER_OFFLINE_ROOT/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin/rustc"
if touch "$WUNDER_OFFLINE_ROOT/.ci-readonly-probe" 2>/dev/null; then
  rm -f "$WUNDER_OFFLINE_ROOT/.ci-readonly-probe"
  echo 'SDK must be mounted read-only' >&2
  exit 2
fi
if curl --connect-timeout 2 --max-time 3 -s http://1.1.1.1 >/dev/null; then exit 2; fi
rustc --version
""", encoding="utf-8")
    for relative in ("packaging/gitea/ci/build.sh", "builders/build-linux-arm64-appimage.sh",
                     "builders/build-linux-amd64-appimage.sh", "builders/build-linux-amd64-offline.sh",
                     "builders/build-cli-linux-arm64-offline.sh", "builders/build-cli-linux-amd64-offline.sh",
                     "builders/linux_arm64_sysroot_cc.sh", "builders/linux-amd64-cross-cc.sh",
                     "config/wunder-example.yaml", "images/pupa.png"):
        destination = directory / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / relative, destination)
    (directory / "Cargo.toml").write_text('''[workspace]
members = ["crates/wunder-cli"]
exclude = ["frontend-slint"]
resolver = "2"
[workspace.package]
version = "0.0.0"
''', encoding="utf-8")
    for relative, package in (("crates/wunder-cli", "wunder-cli"),
                              ("frontend-slint", "wunder-frontend-slint")):
        project = directory / relative
        (project / "src").mkdir(parents=True)
        (project / "Cargo.toml").write_text('[package]\nname = "' + package
                                            + '"\nversion = "0.0.0"\nedition = "2021"\n', encoding="utf-8")
        (project / "src/main.rs").write_text('fn main() { println!("offline probe"); }\n', encoding="utf-8")
        lock = project if relative == "frontend-slint" else directory
        (lock / "Cargo.lock").write_text('version = 4\n[[package]]\nname = "' + package
                                         + '"\nversion = "0.0.0"\n', encoding="utf-8")
    environment = dict(os.environ, GIT_CONFIG_COUNT="1", GIT_CONFIG_KEY_0="http.extraHeader",
                       GIT_CONFIG_VALUE_0="Authorization: Basic " + authorization, GIT_TERMINAL_PROMPT="0")

    def git(*arguments):
        subprocess.run(["git", *arguments], cwd=directory, env=environment, check=True,
                       stdout=subprocess.DEVNULL)

    git("init", "-b", "main")
    git("-c", "user.name=Offline CI", "-c", "user.email=ci@example.invalid", "add", ".")
    git("-c", "user.name=Offline CI", "-c", "user.email=ci@example.invalid", "commit", "-m", "offline stack probe")
    git("remote", "add", "origin", "http://localhost:13000/admin/" + name + ".git")
    git("push", "origin", "main")
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        runs = api("/repos/admin/" + name + "/actions/runs")
        entries = runs.get("workflow_runs", runs.get("runs", []))
        if entries and entries[0].get("status") == "completed":
            if entries[0].get("conclusion") != "success":
                logs = Request("http://localhost:13000/api/v1/repos/admin/" + name
                               + "/actions/runs/" + str(entries[0]["id"]) + "/logs",
                               headers={"Authorization": "Basic " + authorization})
                with urlopen(logs, timeout=30) as response:
                    with zipfile.ZipFile(io.BytesIO(response.read())) as archive:
                        for filename in archive.namelist():
                            print(archive.read(filename).decode(errors="replace")[-16000:])
                raise RuntimeError("offline stack workflow failed: " + name)
            releases = api("/repos/admin/" + name + "/releases")
            assets = {asset["name"] for asset in releases[0]["assets"]}
            expected = {"wunder-slint-0.0.0-linux-" + architecture + ".AppImage"
                        for architecture in ("arm64", "amd64")}
            expected.update("wunder-cli-0.0.0-linux-" + architecture for architecture in ("arm64", "amd64"))
            expected.update("SHA256SUMS-linux-" + architecture + ".txt" for architecture in ("arm64", "amd64"))
            if not expected.issubset(assets):
                raise RuntimeError("both architecture outputs were not published")
            print("PASS: authenticated checkout, push-triggered matrix, ARM64 native/AMD64 cross Rust, "
                  "read-only mounted SDK, original AppImage/CLI build scripts, ELF/glibc checks, "
                  "isolation and local Release uploads")
            return
        time.sleep(3)
    raise TimeoutError("offline stack workflow did not finish")


if __name__ == "__main__":
    main()
