#!/usr/bin/env bash
# Public build entry. Implementation remains in builders/ with the other
# offline toolchain and packaging scripts.
set -euo pipefail
repo_root="$(cd -- "$(dirname -- "$0")" && pwd -P)"
if [[ "${1:-}" == "pack" ]]; then
  shift
  exec bash "$repo_root/builders/pack.sh" "$@"
fi
if [[ "${1:-}" == "portable" ]]; then
  shift
  exec bash "$repo_root/builders/pack-portable.sh" "$@"
fi
exec bash "$repo_root/builders/build.sh" "$@"
