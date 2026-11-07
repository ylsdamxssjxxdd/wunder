#!/usr/bin/env bash
# Compatibility wrapper: Win7 x86 now builds from x86_64 hosts with the
# kylin-x86 portable MinGW toolchain.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
exec "$repo_root/builders/build-win7-x86-offline.sh" "$@"
