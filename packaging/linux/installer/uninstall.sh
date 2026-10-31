#!/bin/sh
# Remove the install created by the Wunder .run installer while keeping user
# data (config, sessions, workspace, logs, skills and vector knowledge).
set -eu

HOME_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ASSUME_YES=0
for argument in "$@"; do
  case "$argument" in
    -y|--yes) ASSUME_YES=1 ;;
    -h|--help)
      echo "用法: $0 [--yes]"
      echo "卸载心舰的蛹和舵机及已安装的补充组件，保留配置、会话和工作区。"
      exit 0
      ;;
    *) echo "未知参数：$argument" >&2; exit 2 ;;
  esac
done

if [ "$ASSUME_YES" -eq 0 ]; then
  printf '卸载 %s 中的心舰？配置、会话和工作区会保留。[y/N] ' "$HOME_DIR"
  if ! IFS= read -r reply; then
    reply=n
  fi
  case "$reply" in
    y|Y) ;;
    *) echo "已取消。"; exit 0 ;;
  esac
fi

if [ -f "$HOME_DIR/bin/install-components.txt" ]; then
  components=$(tr ',' ' ' < "$HOME_DIR/bin/install-components.txt")
  for key in $components; do
    case "$key" in
      python|git|rg|ffmpeg|browser)
        rm -rf -- "$HOME_DIR/opt/$key"
        ;;
    esac
  done
fi

LOCAL_BIN="$HOME/.local/bin"
for name in wunder-cli wunder-desktop desktop; do
  link="$LOCAL_BIN/$name"
  if [ -L "$link" ]; then
    target=$(readlink -- "$link" 2>/dev/null || true)
    case "$target" in
      "$HOME_DIR"/*) rm -f -- "$link" ;;
    esac
  fi
done

profile="$HOME/.profile"
if [ -f "$profile" ] && grep -Fq '# >>> wunder commands >>>' "$profile"; then
  tmp="$profile.wunder.tmp"
  if awk '
    /^# >>> wunder commands >>>$/ { skip = 1; next }
    /^# <<< wunder commands <<<$/ { skip = 0; next }
    !skip { print }
  ' "$profile" > "$tmp"; then
    cat "$tmp" > "$profile"
    rm -f -- "$tmp"
  fi
fi

manifest="$HOME_DIR/bin/install-manifest.txt"
desktop_entry=""
desktop_icon=""
if [ -f "$manifest" ]; then
  desktop_entry=$(sed -n 's/^desktop_entry=//p' "$manifest" | tail -n 1)
  desktop_icon=$(sed -n 's/^desktop_icon=//p' "$manifest" | tail -n 1)
fi
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
if [ -z "$desktop_entry" ]; then
  desktop_entry="$data_home/applications/wunder.desktop"
fi
entry_removed=0
if [ -f "$desktop_entry" ] && grep -Fq 'X-Wunder-Install=1' "$desktop_entry"; then
  rm -f -- "$desktop_entry"
  entry_removed=1
fi
if [ -n "$desktop_icon" ] && [ -f "$desktop_icon" ] &&
  grep -Fq 'X-Wunder-Install=1' "$desktop_icon"; then
  rm -f -- "$desktop_icon"
fi
if [ "$entry_removed" -eq 1 ] && command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$(dirname -- "$desktop_entry")" >/dev/null 2>&1 || true
fi

rm -rf -- "$HOME_DIR/bin"
# The desktop program tree lives outside the user data directories.
rm -rf -- "$HOME_DIR/app/desktop"
rmdir "$HOME_DIR/app" 2>/dev/null || true
rm -f -- "$HOME_DIR/.wunder-home"
rm -f -- "$HOME_DIR/images/pupa.png" "$HOME_DIR/images/wunder.png"
rmdir "$HOME_DIR/images" 2>/dev/null || true

echo "已卸载心舰的蛹、舵机和安装时选择的补充组件。"
echo "保留的用户数据："
echo "  $HOME_DIR/config"
echo "  $HOME_DIR/data"
echo "  $HOME_DIR/workspace"
echo "  $HOME_DIR/logs"
echo "  $HOME_DIR/skills"
echo "  $HOME_DIR/opt（未在本次安装中选择的组件）"
echo "如不再需要，可手动删除：rm -rf -- \"$HOME_DIR\""

rm -f -- "$0"
