#!/usr/bin/env bash
set -euo pipefail

appimage="${1:-}"
[[ -n "$appimage" && -f "$appimage" ]] || { echo "usage: $0 <AppImage>" >&2; exit 2; }
command -v readelf >/dev/null 2>&1 || { echo "readelf is required" >&2; exit 2; }
command -v unsquashfs >/dev/null 2>&1 || { echo "unsquashfs is required" >&2; exit 2; }
echo "[check] $(file "$appimage")"
offset="$(grep -oba 'hsqs' "$appimage" | head -n 1 | cut -d: -f1)"
[[ "$offset" =~ ^[0-9]+$ && "$offset" -gt 0 ]] || { echo "missing SquashFS payload" >&2; exit 1; }
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
dd if="$appimage" of="$tmp/payload.squashfs" bs=1 skip="$offset" status=none
unsquashfs -l "$tmp/payload.squashfs" > "$tmp/list.txt"
for required in \
  "squashfs-root/AppRun" \
  "squashfs-root/usr/bin/wunder-frontend-slint" \
  "squashfs-root/usr/lib/libxcb.so.1" \
  "squashfs-root/usr/lib/libxcb-xkb.so.1" \
  "squashfs-root/usr/lib/libxkbcommon-x11.so.0" \
  "squashfs-root/config/wunder.yaml"; do
  grep -Fq "$required" "$tmp/list.txt" || { echo "missing bundled file: $required" >&2; exit 1; }
done
echo "[check] AppImage payload and required XCB/XKB libraries are present"
