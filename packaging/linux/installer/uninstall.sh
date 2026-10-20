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

rm -rf -- "$HOME_DIR/bin"
rm -f -- "$HOME_DIR/.wunder-home"
rm -f -- "$HOME_DIR/images/wunder.png"
rmdir "$HOME_DIR/images" 2>/dev/null || true

data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
entry="$data_home/applications/wunder.desktop"
if [ -f "$entry" ] && grep -Fq 'X-Wunder-Install=1' "$entry"; then
  rm -f -- "$entry"
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data_home/applications" >/dev/null 2>&1 || true
  fi
fi

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
