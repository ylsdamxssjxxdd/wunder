#!/usr/bin/env bash
# Cross-build Linux ARM64 on an x86_64 Linux host using Docker/buildx.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/.." && pwd -P)"
image="${WUNDER_ARM64_BUILD_IMAGE:-wunder-rust-builder-arm64:ubuntu18}"
docker buildx build --platform linux/arm64 -f "$repo_root/packaging/docker/Dockerfile.ubuntu18-arm64-slint" -t "$image" --load "$repo_root/packaging/docker"
docker run --rm --platform linux/arm64 -v "$repo_root:/workspace" -w /workspace "$image" bash /workspace/builders/build-linux-arm64-offline.sh "$@"
