#!/usr/bin/env bash
set -euo pipefail
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$(cd -- "$(dirname -- "$0")/../../Rust-builder/kylin-arm" && pwd -P)}/offline}"
sdk="$offline_root/linux-arm64-ubuntu18"
root="$sdk/root"
debs="$sdk/debs/devel-arm64"
mkdir -p "$debs" "$root"
cat >/etc/apt/sources.list <<'EOF'
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic main universe multiverse restricted
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic-updates main universe multiverse restricted
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic-security main universe multiverse restricted
EOF
printf 'Acquire::Check-Valid-Until "false";\n' >/etc/apt/apt.conf.d/99wunder-no-valid-until
apt-get update -o Acquire::Check-Valid-Until=false
cd "$debs"
apt-get download libc6:arm64 libc6-dev:arm64 linux-libc-dev:arm64 libgcc1:arm64 libgcc-7-dev:arm64 cpp-7:arm64 gcc-7:arm64 binutils:arm64
for deb in ./*.deb; do dpkg-deb -x "$deb" "$root"; done
echo "prepared ARM64 Ubuntu 18 development sysroot: $root"
