#!/bin/sh
# Wunder self-extracting installer header. The gzip-compressed tar payload
# starts on the line after the marker below; builders append it verbatim.
set -eu

SELF="$0"
if command -v readlink >/dev/null 2>&1; then
  resolved=$(readlink -f -- "$0" 2>/dev/null || true)
  if [ -n "$resolved" ]; then
    SELF="$resolved"
  fi
fi
case "$SELF" in
  /*) ;;
  *) SELF="$(CDPATH= cd -- "$(dirname -- "$SELF")" && pwd)/$(basename -- "$SELF")" ;;
esac

TMP_ROOT="${TMPDIR:-/tmp}"
TMP_DIR=$(mktemp -d "$TMP_ROOT/wunder-install.XXXXXX") || {
  echo "无法创建临时目录：$TMP_ROOT" >&2
  exit 1
}

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  rm -rf -- "$TMP_DIR"
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

PAYLOAD_LINE=$(LC_ALL=C awk '/^__WUNDER_PAYLOAD_BELOW__$/ { print NR + 1; exit 0 }' "$SELF")
if [ -z "$PAYLOAD_LINE" ]; then
  echo "安装包损坏：找不到载荷标记。" >&2
  exit 1
fi

echo "正在解压安装包..."
if ! tail -n +"$PAYLOAD_LINE" "$SELF" | tar -xzf - -C "$TMP_DIR"; then
  echo "安装包解压失败，文件可能不完整。" >&2
  exit 1
fi

WUNDER_PAYLOAD_DIR="$TMP_DIR/payload"
export WUNDER_PAYLOAD_DIR
sh "$TMP_DIR/payload/install.sh" "$@"
exit 0

__WUNDER_PAYLOAD_BELOW__
