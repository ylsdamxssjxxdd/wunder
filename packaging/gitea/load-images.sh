#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
[[ -f .env ]] || cp .env.example .env
env_value() { sed -n "s/^$1=//p" .env | tail -n 1; }
platform="$(env_value CI_PLATFORM)"; platform="${platform:-linux/arm64}"
case "$platform" in
    linux/arm64) expected=aarch64 ;;
    linux/amd64) expected=x86_64 ;;
    *) echo "未知的 CI_PLATFORM: $platform" >&2; exit 2 ;;
esac
[[ "$(uname -m)" == "$expected" ]] || {
    echo "该套件面向 $platform，当前宿主是 $(uname -m)。" >&2
    echo '请在同架构的机器上用 prepare-bundle.py --arch 重新导出。' >&2
    exit 2
}
docker compose version >/dev/null
sha256sum --check images/SHA256SUMS
for archive in images/*.tar.gz; do
    docker load --input "$archive"
done
bash prepare-sdk.sh
printf '%s\n' 'Images loaded and SDK mount prepared. Set the server address in .env, then run: docker compose up -d'
