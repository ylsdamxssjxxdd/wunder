#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
binary="${CARGO_TARGET_DIR:-/tmp/cargo-target}/release/wunder-server"
prefer_prebuilt="${WUNDER_PREFER_PREBUILT_BIN:-0}"

ensure_playwright_browsers() {
  local target="${PLAYWRIGHT_BROWSERS_PATH:-}"
  local seed="${WUNDER_PLAYWRIGHT_SEED_PATH:-/opt/ms-playwright}"

  if [ -z "${target}" ] || [ ! -d "${seed}" ]; then
    return 0
  fi

  mkdir -p "${target}"
  if find "${target}" -maxdepth 1 -type d -name 'chromium*' -print -quit 2>/dev/null | grep -q .; then
    return 0
  fi
  if ! find "${seed}" -maxdepth 1 -type d -name 'chromium*' -print -quit 2>/dev/null | grep -q .; then
    return 0
  fi

  printf '%s\n' "[docker][browser] seeding Playwright Chromium into ${target}" >&2
  cp -a "${seed}/." "${target}/"
}

binary_is_ready() {
  if [ ! -x "${binary}" ]; then
    return 1
  fi

  if [ "${prefer_prebuilt}" = "1" ]; then
    return 0
  fi

  ! find Cargo.toml Cargo.lock crates/wunder-core crates/wunder-runtime crates/wunder-server patches/tokio-xmpp \
    -type f -newer "${binary}" -print -quit 2>/dev/null | grep -q .
}

run_binary() {
  ensure_playwright_browsers
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

    printf '%s\n' "wunder-server missing or stale, waiting for shared build output..." >&2
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

    cargo build --release -p wunder-server --bin wunder-server
    run_binary
    ;;
  *)
    printf 'Usage: %s <wait-or-run|run-or-build>\n' "$0" >&2
    exit 64
    ;;
esac
