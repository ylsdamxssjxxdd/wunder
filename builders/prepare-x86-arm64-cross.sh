#!/usr/bin/env bash
# Ensure the amd64-hosted aarch64 cross compiler from the kylin-x86 SDK is
# unpacked. The SDK ships host tools as one relocatable archive; only the
# cross pieces needed by Rust/Slint builds are extracted here.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd -P)}"
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/kylin-x86}/offline}"
host_dir="$offline_root/host-tools-amd64"
dest="$host_dir/root"
archive="$offline_root/archives/host-tools-amd64-ubuntu18.tar"

fail() { echo "[wunder-x86-arm64-cross] $*" >&2; exit 2; }

[[ "$(uname -m)" == x86_64 ]] || fail "this helper is only needed on an x86_64 Linux host"
if [[ -x "$dest/usr/bin/aarch64-linux-gnu-gcc-7" \
  && -x "$dest/usr/bin/aarch64-linux-gnu-g++-7" \
  && -f "$dest/usr/lib/gcc-cross/aarch64-linux-gnu/7/cc1" \
  && -x "$dest/usr/aarch64-linux-gnu/bin/ld" \
  && -e "$dest/usr/lib/x86_64-linux-gnu/libisl.so.19" \
  && -f "$dest/usr/lib/x86_64-linux-gnu/libbfd-2.30-arm64.so" \
  && -f "$dest/usr/lib/x86_64-linux-gnu/libopcodes-2.30-arm64.so" ]]; then
  printf '%s\n' "$dest"
  exit 0
fi
[[ -f "$archive" ]] || fail "host tools archive is missing and $dest is incomplete: $archive"
command -v tar >/dev/null 2>&1 || fail "tar is required to unpack the host tools archive"

tmp="$host_dir/.root.tmp.$$"
cleanup() { rm -rf -- "$tmp"; }
trap cleanup EXIT
rm -rf -- "$tmp"
mkdir -p "$tmp"

echo "[wunder-x86-arm64-cross] extracting aarch64 cross compiler from $archive" >&2
tar -xf "$archive" -C "$tmp" --wildcards \
  'usr/bin/aarch64-linux-gnu-*' \
  'usr/lib/gcc-cross/aarch64-linux-gnu/7' \
  'usr/aarch64-linux-gnu/bin' \
  'usr/lib/x86_64-linux-gnu/libisl.so.19*' \
  'usr/lib/x86_64-linux-gnu/libmpfr.so.6*' \
  'usr/lib/x86_64-linux-gnu/libmpc.so.3*' \
  'usr/lib/x86_64-linux-gnu/libgmp.so.10*' \
  'usr/lib/x86_64-linux-gnu/libbfd-2.30-arm64.so' \
  'usr/lib/x86_64-linux-gnu/libopcodes-2.30-arm64.so'

[[ -x "$tmp/usr/bin/aarch64-linux-gnu-gcc-7" ]] || fail "cross compiler is missing from the archive"
[[ -f "$tmp/usr/lib/gcc-cross/aarch64-linux-gnu/7/cc1" ]] || fail "cross compiler backend is missing from the archive"
[[ -x "$tmp/usr/aarch64-linux-gnu/bin/ld" ]] || fail "cross linker is missing from the archive"

rm -rf -- "$dest"
mv "$tmp" "$dest"
printf '%s\n' "$dest"
