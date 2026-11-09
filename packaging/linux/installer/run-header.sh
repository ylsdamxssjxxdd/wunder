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

# 进度条：默认开启，输出到 stderr，所以重定向 stdout 也不会污染摘要。
# 终端里用回车原地刷新；非终端（日志、管道）按 10% 一档换行输出。
WUNDER_PROGRESS=1
for arg in "$@"; do
  case "$arg" in
    --no-progress) WUNDER_PROGRESS=0 ;;
    --progress) WUNDER_PROGRESS=1 ;;
  esac
done
if [ -n "${NO_COLOR:-}" ]; then
  WUNDER_PROGRESS=0
fi
if [ "$WUNDER_PROGRESS" -eq 1 ] && [ -t 2 ]; then
  LIVE=1
else
  LIVE=0
fi
export WUNDER_PROGRESS

# 载荷解压占比 55%，安装与组件解压 45%，两者合计对应安装包总字节数。
PAYLOAD_WEIGHT=55
TOTAL_BYTES=$(wc -c < "$SELF" | tr -d ' ')
TOTAL_BYTES=${TOTAL_BYTES:-0}
LAST_TICK=""
PROGRESS_OPEN=0

human_size() {
  bytes=${1:-0}
  case "$bytes" in
    ''|*[!0-9]*) bytes=0 ;;
  esac
  if [ "$bytes" -ge 1073741824 ]; then
    awk -v b="$bytes" 'BEGIN { printf "%.1f GB", b / 1073741824 }'
  elif [ "$bytes" -ge 1048576 ]; then
    awk -v b="$bytes" 'BEGIN { printf "%.0f MB", b / 1048576 }'
  else
    awk -v b="$bytes" 'BEGIN { printf "%.0f KB", b / 1024 }'
  fi
}

bar() {
  width=$1
  percent=$2
  filled=$((percent * width / 100))
  [ "$filled" -gt "$width" ] && filled=$width
  out=""
  i=0
  while [ "$i" -lt "$width" ]; do
    if [ "$i" -lt "$filled" ]; then out="$out#"; else out="$out-"; fi
    i=$((i + 1))
  done
  printf '%s' "$out"
}

# draw <百分比> <阶段文字>
draw() {
  percent=$1
  label=$2
  [ "$percent" -gt 100 ] && percent=100
  [ "$percent" -lt 0 ] && percent=0
  if [ "$LIVE" -eq 1 ]; then
    printf '\r\033[K  [%s] %3d%%  %s' "$(bar 24 "$percent")" "$percent" "$label" >&2
    PROGRESS_OPEN=1
  else
    # 非终端：只按 10% 档位换行打印，避免日志刷屏。
    tick=$((percent / 10))
    if [ "$tick" != "$LAST_TICK" ]; then
      LAST_TICK=$tick
      printf '  [%s] %3d%%  %s\n' "$(bar 24 "$percent")" "$percent" "$label" >&2
    fi
  fi
}

# finish_progress：把光标移到进度行之后，避免后续输出粘在进度条上。
finish_progress() {
  if [ "$PROGRESS_OPEN" -eq 1 ]; then
    printf '\n' >&2
    PROGRESS_OPEN=0
  fi
}

# 把 "阶段 已完成 总量" 换算成总进度百分比。
stage_percent() {
  base=$1
  weight=$2
  done_bytes=$3
  total=$4
  if [ "$total" -le 0 ]; then
    printf '%s' "$base"
    return
  fi
  [ "$done_bytes" -gt "$total" ] && done_bytes=$total
  printf '%s' $((base + done_bytes * weight / total))
}

# 后台进度监视：payload 解压期间按已落盘字节推进。
watch_extract() {
  while [ ! -f "$TMP_DIR/.extract-done" ]; do
    used=$(du -sb --apparent-size "$TMP_DIR" 2>/dev/null | cut -f1)
    used=${used:-0}
    draw "$(stage_percent 0 "$PAYLOAD_WEIGHT" "$used" "$TOTAL_BYTES")" \
      "解压安装包 $(human_size "$used") / $(human_size "$TOTAL_BYTES")"
    sleep 1
  done
}

echo "正在解压安装包…"
if [ "$WUNDER_PROGRESS" -eq 1 ]; then
  watch_extract &
  WATCH_PID=$!
fi
if ! tail -n +"$PAYLOAD_LINE" "$SELF" | tar -xzf - -C "$TMP_DIR"; then
  [ -n "${WATCH_PID:-}" ] && kill "$WATCH_PID" 2>/dev/null || true
  finish_progress
  echo "安装包解压失败，文件可能不完整。" >&2
  exit 1
fi
if [ -n "${WATCH_PID:-}" ]; then
  : > "$TMP_DIR/.extract-done"
  wait "$WATCH_PID" 2>/dev/null || true
  WATCH_PID=""
fi
if [ "$WUNDER_PROGRESS" -eq 1 ]; then
  draw "$PAYLOAD_WEIGHT" "解压安装包 $(human_size "$TOTAL_BYTES") / $(human_size "$TOTAL_BYTES")"
  finish_progress
fi

WUNDER_PAYLOAD_DIR="$TMP_DIR/payload"
export WUNDER_PAYLOAD_DIR
export WUNDER_PROGRESS_BASE="$PAYLOAD_WEIGHT"
export WUNDER_PROGRESS_WEIGHT=$((100 - PAYLOAD_WEIGHT))
export WUNDER_PROGRESS_LIVE="$LIVE"
sh "$TMP_DIR/payload/install.sh" "$@"
exit 0

__WUNDER_PAYLOAD_BELOW__
