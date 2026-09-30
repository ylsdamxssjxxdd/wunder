#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
builder_root="${RUST_BUILDER_ROOT:-$repo_root/../Rust-builder}"
output="${KYLIN_X86_OUTPUT:-$builder_root/kylin-x86}"
image="${KYLIN_X86_IMAGE:-wunder-rust-builder-kylin-x86:ubuntu18}"
mkdir -p "$output/offline/archives"
docker build --platform linux/amd64 -f "$repo_root/packaging/docker/Dockerfile.rust-builder-kylin-x86" -t "$image" "$repo_root"
docker run --rm --network none --platform linux/amd64 --entrypoint bash \
  -e SOURCE_PROFILE=/source -e OUTPUT_PROFILE=/output \
  -v "$builder_root/kylin-arm:/source:ro" -v "$output:/output" \
  -v "$repo_root/builders:/scripts:ro" "$image" /scripts/prepare-kylin-x86-sdk.sh
docker image save --output "$output/offline/archives/builder-amd64-ubuntu18.docker.tar" "$image"
docker run --rm --network none --platform linux/amd64 --entrypoint bash \
  -v "$output:/sdk" -v "$repo_root/builders:/scripts:ro" "$image" /scripts/check-kylin-x86-sdk.sh
python3 "$repo_root/builders/index-kylin-x86-sdk.py" "$output"
