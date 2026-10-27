#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
[[ "$(uname -m)" == aarch64 || "$(uname -m)" == arm64 ]] || {
    echo 'This offline deployment is for ARM64 Linux.' >&2; exit 2;
}
docker compose version >/dev/null
sha256sum --check images/SHA256SUMS
for archive in images/*.tar.gz; do
    docker load --input "$archive"
done
bash prepare-sdk.sh
printf '%s\n' 'Images loaded and SDK mount prepared. Set the server address in .env, then run: docker compose up -d'
