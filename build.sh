#!/usr/bin/env bash
# Public build entry. Implementation remains in builders/ with the other
# offline toolchain and packaging scripts.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
exec bash "$repo_root/builders/build.sh" "$@"
