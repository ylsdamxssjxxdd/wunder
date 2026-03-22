#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
binary="${CARGO_TARGET_DIR:-/tmp/cargo-target}/release/wunder-server"
prefer_prebuilt="${WUNDER_PREFER_PREBUILT_BIN:-0}"

binary_is_ready() {
  if [ ! -x "${binary}" ]; then
    return 1
  fi

  if [ "${prefer_prebuilt}" = "1" ]; then
    return 0
  fi

  ! find src Cargo.toml Cargo.lock -type f -newer "${binary}" -print -quit 2>/dev/null | grep -q .
}

run_binary() {
  exec "${binary}"
}

if [ -x "${binary}" ] && [ "${prefer_prebuilt}" = "1" ]; then
  printf '%s\n' "[docker][rust] reusing prebuilt release binary because WUNDER_PREFER_PREBUILT_BIN=1" >&2
fi

case "${mode}" in
  wait-or-run)
    if binary_is_ready; then
      run_binary
    fi

    printf '%s\n' "wunder-server missing or stale, waiting for sandbox build..." >&2
    while true; do
      if binary_is_ready; then
        run_binary
      fi
      sleep 1
    done
    ;;
  run-or-build)
    if binary_is_ready; then
      run_binary
    fi

    cargo build --release --bin wunder-server
    run_binary
    ;;
  *)
    printf 'Usage: %s <wait-or-run|run-or-build>\n' "$0" >&2
    exit 64
    ;;
esac
