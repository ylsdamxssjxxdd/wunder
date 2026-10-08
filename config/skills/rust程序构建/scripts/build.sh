#!/usr/bin/env bash
set -euo pipefail
TARGET=${TARGET:-win7-x86}; SDK_ROOT=${SDK_ROOT:-${RUST_BUILDER_ROOT:-$(cd .. && pwd)/Rust-builder}}
python3 "$(dirname "$0")/preflight.py" --target "$TARGET" --sdk-root "$SDK_ROOT"
case "$TARGET" in win7-x86) TRIPLE=i686-win7-windows-gnu;; linux-amd64-ubuntu18|kylin-x86) TRIPLE=x86_64-unknown-linux-gnu;; linux-arm64-ubuntu18) TRIPLE=aarch64-unknown-linux-gnu;; *) echo "unknown target" >&2; exit 2;; esac
export CARGO_NET_OFFLINE=true
cargo build --locked --offline --release --target "$TRIPLE" "$@"
