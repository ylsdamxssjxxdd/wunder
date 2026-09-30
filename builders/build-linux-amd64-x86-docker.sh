#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
builder_root="${RUST_BUILDER_ROOT:-$repo_root/../Rust-builder}"
image="${WUNDER_AMD64_BUILD_IMAGE:-wunder-rust-builder-kylin-x86:ubuntu18}"
docker run --rm --network none --entrypoint bash --platform linux/amd64 -v "$repo_root:/workspace" -v "$builder_root:/Rust-builder:ro" -w /workspace "$image" -lc 'export WUNDER_BUILDER_ROOT=/Rust-builder/kylin-x86 && bash builders/build-linux-amd64-offline.sh'
