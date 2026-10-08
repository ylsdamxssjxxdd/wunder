#!/usr/bin/env bash
set -euo pipefail

appimage="${1:-}"
[[ -n "$appimage" && -f "$appimage" ]] || { echo "usage: $0 <AppImage>" >&2; exit 2; }
command -v readelf >/dev/null 2>&1 || { echo "readelf is required" >&2; exit 2; }
command -v unsquashfs >/dev/null 2>&1 || { echo "unsquashfs is required" >&2; exit 2; }
echo "[check] $(file "$appimage")"
# The runtime ELF may carry look-alike "hsqs" bytes, so the first hit is not
# necessarily the payload: scan every candidate and keep the first whose
# squashfs 4.0 superblock actually parses (version 4.0, sane power-of-two
# block size). This mirrors the runtime-cutting logic in the packaging
# scripts under builders/.
payload_offset=""
while IFS=: read -r candidate _; do
  [[ "$candidate" =~ ^[0-9]+$ ]] || continue
  block_size="$(dd if="$appimage" bs=8192 skip=$((candidate + 12)) count=4 iflag=skip_bytes,count_bytes status=none | od -An -tu4 | tr -d ' ')"
  sb_major="$(dd if="$appimage" bs=8192 skip=$((candidate + 28)) count=2 iflag=skip_bytes,count_bytes status=none | od -An -tu2 | tr -d ' ')"
  sb_minor="$(dd if="$appimage" bs=8192 skip=$((candidate + 30)) count=2 iflag=skip_bytes,count_bytes status=none | od -An -tu2 | tr -d ' ')"
  [[ "$block_size" =~ ^[0-9]+$ && "$sb_major" =~ ^[0-9]+$ && "$sb_minor" =~ ^[0-9]+$ ]] || continue
  if [[ "$sb_major" == 4 && "$sb_minor" == 0 ]] \
    && (( block_size >= 4096 && block_size <= 1048576 && (block_size & (block_size - 1)) == 0 )); then
    payload_offset="$candidate"
    break
  fi
done < <(LC_ALL=C grep -oba 'hsqs' "$appimage" || true)
[[ -n "$payload_offset" && "$payload_offset" -gt 0 ]] || { echo "no valid SquashFS 4.0 payload found in AppImage" >&2; exit 1; }
echo "[check] payload offset: $payload_offset"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
dd if="$appimage" of="$tmp/payload.squashfs" bs=1M iflag=skip_bytes skip="$payload_offset" status=none
unsquashfs -l "$tmp/payload.squashfs" > "$tmp/list.txt"
for required in \
  "squashfs-root/AppRun" \
  "squashfs-root/usr/bin/wunder-frontend-slint" \
  "squashfs-root/usr/lib/libX11.so.6" \
  "squashfs-root/usr/lib/libXtst.so.6" \
  "squashfs-root/usr/lib/libxcb.so.1" \
  "squashfs-root/usr/lib/libxcb-xkb.so.1" \
  "squashfs-root/usr/lib/libxkbcommon.so.0" \
  "squashfs-root/usr/lib/libxkbcommon-x11.so.0" \
  "squashfs-root/usr/lib/libasound.so.2" \
  "squashfs-root/config/wunder.yaml"; do
  grep -Fq "$required" "$tmp/list.txt" || { echo "missing bundled file: $required" >&2; exit 1; }
done
echo "[check] AppImage payload and required X11/XTest/XCB/XKB/ALSA libraries are present"
