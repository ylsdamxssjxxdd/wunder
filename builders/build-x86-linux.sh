#!/usr/bin/env bash
# x86_64 Linux host: build native amd64, cross ARM64, or cross Win7 from offline SDKs.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
mode="${1:-amd64}"
case "$mode" in
 amd64|linux-amd64) exec "$repo_root/builders/build-linux-amd64-offline.sh" "${@:2}" ;;
 arm64|linux-arm64) exec "$repo_root/builders/build-linux-arm64-offline.sh" "${@:2}" ;;
 win7|win7-x86) exec "$repo_root/builders/build-win7-x86-from-amd64.sh" "${@:2}" ;;
 *) echo "usage: $0 {amd64|arm64|win7}" >&2; exit 2;;
esac
