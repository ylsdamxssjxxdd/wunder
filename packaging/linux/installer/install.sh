#!/bin/sh
# Wunder Linux installer body. It is embedded in the .run payload and always
# receives WUNDER_PAYLOAD_DIR pointing at the extracted payload directory.
set -eu
umask 022

PAYLOAD_DIR="${WUNDER_PAYLOAD_DIR:-$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)}"
VERSION=$(cat "$PAYLOAD_DIR/version" 2>/dev/null || echo "unknown")
ARCH=$(cat "$PAYLOAD_DIR/arch" 2>/dev/null || echo "unknown")
[ -n "${HOME:-}" ] || {
  echo "HOME 未设置，无法确定默认安装目录。" >&2
  exit 1
}
HOME_DIR="${WUNDER_HOME:-$HOME/.wunder}"
HOME_GIVEN=0
if [ -n "${WUNDER_HOME:-}" ]; then
  HOME_GIVEN=1
fi
COMPONENTS_FILE="$PAYLOAD_DIR/components.txt"
SELECTED_FILE="$PAYLOAD_DIR/../.selected"
ASSUME_YES=0
SELECT_ALL=0
COMPONENTS_GIVEN=0
COMPONENTS_ARG=""
DESKTOP_ENTRY=1

die() {
  echo "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
用法: wunder-<版本>-linux-<架构>.run [选项]

  -y, --yes                不询问确认
      --components=LIST    只安装列出的补充组件，逗号分隔；例如 python,git
      --all                安装全部补充组件
      --home=PATH          安装目录，同时保存用户数据，默认 ${WUNDER_HOME:-$HOME/.wunder}
      --no-desktop-entry   不创建应用菜单项
  -h, --help               显示帮助

不指定 --components/--all 时默认不安装任何补充组件。交互模式会显示菜单。
EOF
}

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

component_path() {
  awk -F'|' -v key="$1" '$1 == key { print $3; exit }' "$COMPONENTS_FILE"
}

component_label() {
  awk -F'|' -v key="$1" '$1 == key { print $2; exit }' "$COMPONENTS_FILE"
}

is_selected() {
  grep -Fxq -- "$1" "$SELECTED_FILE" 2>/dev/null
}

select_one() {
  is_selected "$1" || printf '%s\n' "$1" >> "$SELECTED_FILE"
}

unselect_one() {
  grep -Fvx -- "$1" "$SELECTED_FILE" > "$SELECTED_FILE.tmp" 2>/dev/null || true
  mv -f -- "$SELECTED_FILE.tmp" "$SELECTED_FILE"
}

selected_count() {
  grep -c . "$SELECTED_FILE" 2>/dev/null || true
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    -y|--yes) ASSUME_YES=1 ;;
    --all) SELECT_ALL=1 ;;
    --components=*) COMPONENTS_ARG=${1#*=}; COMPONENTS_GIVEN=1 ;;
    --components)
      shift
      [ "$#" -gt 0 ] || die "--components 需要一个值"
      COMPONENTS_ARG=$1
      COMPONENTS_GIVEN=1
      ;;
    --home=*) HOME_DIR=${1#*=}; HOME_GIVEN=1 ;;
    --home)
      shift
      [ "$#" -gt 0 ] || die "--home 需要一个值"
      HOME_DIR=$1
      HOME_GIVEN=1
      ;;
    --no-desktop-entry) DESKTOP_ENTRY=0 ;;
    -h|--help) usage; exit 0 ;;
    *) die "未知参数：$1" ;;
  esac
  shift
done

case "$HOME_DIR" in
  "~") HOME_DIR=$HOME ;;
  "~/"*) HOME_DIR="$HOME/${HOME_DIR#~/}" ;;
esac
if [ "$HOME_GIVEN" -eq 0 ] && [ "$ASSUME_YES" -eq 0 ] && [ -t 0 ]; then
  echo
  echo "安装目录同时保存配置、会话和工作区等用户数据。"
  printf '安装目录 [%s]（回车使用默认，或输入绝对路径）：' "$HOME_DIR"
  if IFS= read -r chosen; then
    [ -n "$chosen" ] && HOME_DIR=$chosen
  fi
  case "$HOME_DIR" in
    "~") HOME_DIR=$HOME ;;
    "~/"*) HOME_DIR="$HOME/${HOME_DIR#~/}" ;;
  esac
fi
case "$HOME_DIR" in
  /*) ;;
  *) die "安装目录必须是绝对路径：$HOME_DIR" ;;
esac
if [ "$HOME_DIR" = "/" ]; then
  die "拒绝安装到 /"
fi
HOME_DIR=${HOME_DIR%/}

[ -f "$COMPONENTS_FILE" ] || die "安装包缺少组件清单。"
: > "$SELECTED_FILE"

if [ "$COMPONENTS_GIVEN" -eq 1 ] && [ "$SELECT_ALL" -eq 1 ]; then
  die "--components 与 --all 不能同时使用"
fi

if [ "$COMPONENTS_GIVEN" -eq 1 ]; then
  old_ifs=$IFS
  IFS=,
  for key in $COMPONENTS_ARG; do
    key=$(printf '%s' "$key" | tr -d '[:space:]')
    [ -n "$key" ] || continue
    if ! cut -d'|' -f1 "$COMPONENTS_FILE" | grep -Fxq -- "$key"; then
      IFS=$old_ifs
      die "未知组件：$key"
    fi
    select_one "$key"
  done
  IFS=$old_ifs
elif [ "$SELECT_ALL" -eq 1 ]; then
  cut -d'|' -f1 "$COMPONENTS_FILE" | while IFS= read -r key; do
    [ -n "$key" ] && printf '%s\n' "$key"
  done > "$SELECTED_FILE"
elif [ "$ASSUME_YES" -eq 0 ]; then
  [ -t 0 ] || die "非交互环境请使用 --yes --components=... 或 --all"
  while :; do
    echo
    echo "心舰 Wunder $VERSION ($ARCH)"
    echo "安装目录：$HOME_DIR"
    echo
    if [ ! -s "$COMPONENTS_FILE" ]; then
      echo "此安装包不包含补充组件。"
    else
      index=0
      while IFS='|' read -r key label path version bytes files; do
        [ -n "$key" ] || continue
        index=$((index + 1))
        mark=' '
        is_selected "$key" && mark='x'
        printf '  %2d) [%s] %s (%s)\n' "$index" "$mark" "$label" "$(human_size "$bytes")"
      done < "$COMPONENTS_FILE"
      echo
      echo "输入编号切换勾选，a 全选，n 全不选，回车继续，q 退出。"
    fi
    printf '选择：'
    if ! IFS= read -r answer; then
      answer=q
    fi
    case "$answer" in
      '') break ;;
      q|Q) exit 0 ;;
      a|A)
        cut -d'|' -f1 "$COMPONENTS_FILE" | while IFS= read -r key; do
          [ -n "$key" ] && printf '%s\n' "$key"
        done > "$SELECTED_FILE"
        ;;
      n|N) : > "$SELECTED_FILE" ;;
      *[!0-9]*) echo "请输入编号。" ;;
      *)
        key=$(awk -F'|' -v n="$answer" 'NF { i++; if (i == n) { print $1; exit } }' "$COMPONENTS_FILE")
        if [ -z "$key" ]; then
          echo "编号超出范围。"
        elif is_selected "$key"; then
          unselect_one "$key"
        else
          select_one "$key"
        fi
        ;;
    esac
  done
fi

count=$(selected_count)
echo
echo "安装目录：$HOME_DIR（同时作为用户数据目录）"
echo "蛹 + 舵机：将安装"
if [ "$count" -gt 0 ]; then
  echo "补充组件："
  while IFS= read -r key; do
    [ -n "$key" ] && echo "  - $(component_label "$key")"
  done < "$SELECTED_FILE"
else
  echo "补充组件：不安装"
fi

if [ "$ASSUME_YES" -eq 0 ]; then
  printf '继续安装？[Y/n] '
  if ! IFS= read -r reply; then
    reply=y
  fi
  case "$reply" in
    n|N) echo "已取消。"; exit 0 ;;
  esac
fi

existing=""
while IFS= read -r key; do
  [ -n "$key" ] || continue
  path=$(component_path "$key")
  [ -d "$HOME_DIR/$path" ] && existing="$existing $key"
done < "$SELECTED_FILE"
if [ -n "$existing" ]; then
  echo "检测到已安装组件：$existing"
  echo "安装前会先清空这些组件的目录；配置、会话和工作区不会被删除。"
  if [ "$ASSUME_YES" -eq 0 ]; then
    printf '继续并清空这些目录？[Y/n] '
    if ! IFS= read -r reply; then
      reply=y
    fi
    case "$reply" in
      n|N) echo "已取消。"; exit 0 ;;
    esac
  fi
fi

mkdir -p "$HOME_DIR/bin" "$HOME_DIR/opt"

while IFS= read -r key; do
  [ -n "$key" ] || continue
  path=$(component_path "$key")
  if [ -d "$HOME_DIR/$path" ]; then
    echo "清理旧组件：$path"
    rm -rf -- "$HOME_DIR/$path"
  fi
done < "$SELECTED_FILE"

echo "安装蛹和舵机..."
cp -f -- "$PAYLOAD_DIR/wunder-desktop.AppImage" "$HOME_DIR/bin/wunder-desktop.AppImage"
cp -f -- "$PAYLOAD_DIR/wunder" "$HOME_DIR/bin/wunder"
chmod 0755 "$HOME_DIR/bin/wunder-desktop.AppImage" "$HOME_DIR/bin/wunder"

cat > "$HOME_DIR/bin/wunder-desktop" <<'LAUNCHER'
#!/bin/sh
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
APP="$DIR/wunder-desktop.AppImage"
ROOT=$(CDPATH= cd -- "$DIR/.." && pwd)
if [ -z "${WUNDER_HOME:-}" ]; then
  WUNDER_HOME="$ROOT"
  export WUNDER_HOME
fi
if command -v fusermount >/dev/null 2>&1 || command -v fusermount3 >/dev/null 2>&1; then
  exec "$APP" "$@"
fi
exec "$APP" --appimage-extract-and-run "$@"
LAUNCHER
chmod 0755 "$HOME_DIR/bin/wunder-desktop"

if [ -f "$PAYLOAD_DIR/wunder.png" ]; then
  mkdir -p "$HOME_DIR/images"
  cp -f -- "$PAYLOAD_DIR/wunder.png" "$HOME_DIR/images/wunder.png"
fi

if [ "$count" -gt 0 ]; then
  members=""
  while IFS= read -r key; do
    [ -n "$key" ] || continue
    path=$(component_path "$key")
    [ -n "$path" ] || die "组件清单缺少路径：$key"
    members="$members $path"
  done < "$SELECTED_FILE"
  echo "安装补充组件..."
  if ! tar -xzf "$PAYLOAD_DIR/supplement.tar.gz" -C "$HOME_DIR" --no-same-owner $members; then
    echo "补充组件解压失败。请检查磁盘空间后重新运行安装包；再次安装会先清理本次留下的目录。" >&2
    exit 1
  fi
  while IFS= read -r key; do
    path=$(component_path "$key")
    [ -d "$HOME_DIR/$path" ] || die "组件未正确解压：$key"
  done < "$SELECTED_FILE"
fi

components_csv=$(awk 'NF { if (out != "") out = out "," $0; else out = $0 } END { print out }' "$SELECTED_FILE")
printf '%s\n' "$components_csv" > "$HOME_DIR/bin/install-components.txt"
printf 'portable\n' > "$HOME_DIR/.wunder-home"
installed_at=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
cat > "$HOME_DIR/bin/install-manifest.txt" <<EOF
version=$VERSION
arch=$ARCH
home=$HOME_DIR
components=$components_csv
installed_at=$installed_at
EOF

if [ "$DESKTOP_ENTRY" -eq 1 ]; then
  data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
  entry_dir="$data_home/applications"
  mkdir -p "$entry_dir"
  escaped_exec=$(printf '%s' "$HOME_DIR/bin/wunder-desktop" | sed 's/\\/\\\\/g; s/"/\\"/g')
  icon="$HOME_DIR/images/wunder.png"
  [ -f "$icon" ] || icon="wunder"
  cat > "$entry_dir/wunder.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=心舰
Comment=本地 AI 工作台
Exec="$escaped_exec" %U
Icon=$icon
Terminal=false
Categories=Utility;
X-Wunder-Install=1
EOF
  chmod 0644 "$entry_dir/wunder.desktop"
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$entry_dir" >/dev/null 2>&1 || true
  fi
fi

cp -f -- "$PAYLOAD_DIR/uninstall.sh" "$HOME_DIR/uninstall.sh"
chmod 0755 "$HOME_DIR/uninstall.sh"

echo
echo "安装完成：$HOME_DIR"
echo "蛹：$HOME_DIR/bin/wunder-desktop（应用菜单中的“心舰”）"
echo "舵机：$HOME_DIR/bin/wunder"
echo "卸载：$HOME_DIR/uninstall.sh"
if [ "$count" -gt 0 ]; then
  echo "补充组件：$components_csv"
else
  echo "补充组件：未安装；可重新运行安装包并勾选。"
fi
