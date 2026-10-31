#!/bin/sh
# Wunder portable launcher. The gzip-compressed AppDir archive starts on the
# line after the marker at the end of this header; builders append it verbatim.
#
# The first run unpacks the archive into <home>/app/desktop and starts it, so
# the target machine needs neither FUSE, nor libfuse2, nor an AppImage runtime.
# Later runs compare the stamped version and start directly, which makes the
# second launch faster than an AppImage mount.
#
# The layout is identical to what the .run installer produces, so a portable
# install and an installed one can replace each other without touching user
# data: config, sessions, workspace, logs and opt/ all live outside app/.
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

PORTABLE_VERSION='@VERSION@'
ROOT="${WUNDER_HOME:-$HOME/.wunder}"
case "$ROOT" in
  /*) ;;
  *) echo "WUNDER_HOME 必须是绝对路径：$ROOT" >&2; exit 1 ;;
esac
ROOT=${ROOT%/}
case "$ROOT" in
  ""|/) echo "拒绝把程序解压到 $ROOT" >&2; exit 1 ;;
esac
APP="$ROOT/app/desktop"
# Both this launcher and the engine inside the archive must agree on the home
# before any runtime path is resolved.
export WUNDER_HOME="$ROOT"

if [ -f "$APP/.portable-version" ] &&
  [ "$(cat "$APP/.portable-version" 2>/dev/null || true)" = "$PORTABLE_VERSION" ]; then
  exec "$APP/AppRun" "$@"
fi

PAYLOAD_LINE=$(LC_ALL=C awk '/^__WUNDER_PORTABLE_PAYLOAD__$/ { print NR + 1; exit 0 }' "$SELF")
if [ -z "$PAYLOAD_LINE" ]; then
  echo "文件损坏：找不到载荷标记。" >&2
  exit 1
fi

echo "首次启动，正在解压到 $APP …"
mkdir -p "$ROOT/app"
staging=$(mktemp -d "$ROOT/app/.desktop.XXXXXX") || {
  echo "无法在 $ROOT/app 创建临时目录。" >&2
  exit 1
}

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  if [ -n "$staging" ]; then
    rm -rf -- "$staging"
  fi
  exit "$status"
}
trap cleanup EXIT HUP INT TERM

# Unpack beside the target and swap in, so an interrupted first run never
# leaves a half-written program directory behind.
if ! tail -n +"$PAYLOAD_LINE" "$SELF" | tar -xzf - -C "$staging" --no-same-owner; then
  echo "解压失败，文件可能不完整或磁盘空间不足。" >&2
  exit 1
fi
if [ ! -x "$staging/AppRun" ]; then
  echo "载荷缺少启动入口。" >&2
  exit 1
fi
printf '%s' "$PORTABLE_VERSION" > "$staging/.portable-version"

rm -rf -- "$APP"
mv -- "$staging" "$APP"
staging=""
# Same marker the installers write, so the CLI resolves this directory as its
# home too.
printf 'portable\n' > "$ROOT/.wunder-home"

trap - EXIT HUP INT TERM
exec "$APP/AppRun" "$@"

exit 0

__WUNDER_PORTABLE_PAYLOAD__
