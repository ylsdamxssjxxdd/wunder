#!/usr/bin/env bash
# Compatibility wrapper: the root build.sh now detects x86_64 hosts itself and
# selects the kylin-x86 SDK. These shortcuts build the Slint desktop.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
mode="${1:-x64}"
shift || true
case "$mode" in
 x64|linux-x64) exec "$repo_root/build.sh" -t desktop -a linux-x64 --native "$@" ;;
 arm64|linux-arm64) exec "$repo_root/build.sh" -t desktop -a linux-arm64 --native "$@" ;;
 win7|win7-x86) exec "$repo_root/build.sh" -t desktop -a win7-x86 --native "$@" ;;
 *) echo "usage: $0 {x64|arm64|win7}" >&2; exit 2;;
esac
