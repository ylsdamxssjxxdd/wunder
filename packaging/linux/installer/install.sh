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
COMPONENTS_FILE="$PAYLOAD_DIR/components.txt"
SELECTED_FILE="$PAYLOAD_DIR/../.selected"
ASSUME_YES=0
SELECT_ALL=0
COMPONENTS_GIVEN=0
COMPONENTS_ARG=""
DESKTOP_ENTRY=1
DESKTOP_ICON=1

die() {
  echo "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
用法: wunder-<版本>-linux-<架构>.run [选项]

直接运行即可：默认装到 ${WUNDER_HOME:-$HOME/.wunder}，只安装蛹和舵机。
交互模式只有一屏：勾选补充组件后回车即开始安装。

  -y, --yes                不询问确认
      --components=LIST    只安装列出的补充组件，逗号分隔；例如 python,git
      --all                安装全部补充组件
      --home=PATH          安装目录，同时保存用户数据，默认 ${WUNDER_HOME:-$HOME/.wunder}
      --no-desktop-entry   不创建应用菜单项
      --no-desktop-icon    不在桌面创建启动图标
  -h, --help               显示帮助

不指定 --components/--all 时默认不安装任何补充组件。
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

write_wunder_desktop_file() {
  target=$1
  mode=$2
  mkdir -p "$(dirname -- "$target")" || return 1
  cat > "$target" <<EOF
[Desktop Entry]
Type=Application
Name=蛹
Comment=本地 AI 工作台
Exec="$escaped_exec" %U
Icon=$icon
Terminal=false
Categories=Utility;
X-Wunder-Install=1
EOF
  chmod "$mode" "$target"
}

resolve_desktop_dir() {
  desktop_dir=""
  if command -v xdg-user-dir >/dev/null 2>&1; then
    desktop_dir=$(xdg-user-dir DESKTOP 2>/dev/null || true)
  fi
  case "$desktop_dir" in
    ""|"$HOME"|"$HOME/"|"/") desktop_dir="" ;;
  esac
  if [ -z "$desktop_dir" ]; then
    user_dirs="${XDG_CONFIG_HOME:-$HOME/.config}/user-dirs.dirs"
    if [ -f "$user_dirs" ]; then
      desktop_dir=$(sed -n 's/^XDG_DESKTOP_DIR="\(.*\)"$/\1/p' "$user_dirs" | tail -n 1)
      case "$desktop_dir" in
        '$HOME'/*) desktop_dir="$HOME/${desktop_dir#\$HOME/}" ;;
      esac
    fi
  fi
  case "$desktop_dir" in
    ""|"$HOME"|"$HOME/"|"/") desktop_dir="$HOME/Desktop" ;;
  esac
  printf '%s\n' "$desktop_dir"
}

component_path() {
  awk -F'|' -v key="$1" '$1 == key { print $3; exit }' "$COMPONENTS_FILE"
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
    --home=*) HOME_DIR=${1#*=} ;;
    --home)
      shift
      [ "$#" -gt 0 ] || die "--home 需要一个值"
      HOME_DIR=$1
      ;;
    --no-desktop-entry) DESKTOP_ENTRY=0 ;;
    --no-desktop-icon) DESKTOP_ICON=0 ;;
    -h|--help) usage; exit 0 ;;
    *) die "未知参数：$1" ;;
  esac
  shift
done

case "$HOME_DIR" in
  "~") HOME_DIR=$HOME ;;
  "~/"*) HOME_DIR="$HOME/${HOME_DIR#~/}" ;;
esac
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
  # One screen, one decision: the menu shows the target directory and the
  # current selection, and Enter starts the install. There is no second
  # summary and no extra confirmation for a first install.
  if [ -s "$COMPONENTS_FILE" ]; then
    printf '\n心舰 Wunder %s · %s\n' "$VERSION" "$ARCH"
    printf '安装到 %s（配置、会话和工作区也在这里；--home=PATH 可改）\n' "$HOME_DIR"
    while :; do
      printf '\n'
      index=0
      while IFS='|' read -r key label path version bytes files; do
        [ -n "$key" ] || continue
        index=$((index + 1))
        mark=' '
        is_selected "$key" && mark='x'
        printf '  %2d) [%s] %s  %s\n' "$index" "$mark" "$label" "$(human_size "$bytes")"
      done < "$COMPONENTS_FILE"
      printf '\n数字切换，a 全选，n 清空，回车开始安装，q 退出\n> '
      if ! IFS= read -r answer; then
        answer=q
      fi
      case "$answer" in
        '') break ;;
        q|Q) echo "已取消。"; exit 0 ;;
        a|A)
          cut -d'|' -f1 "$COMPONENTS_FILE" | while IFS= read -r key; do
            [ -n "$key" ] && printf '%s\n' "$key"
          done > "$SELECTED_FILE"
          ;;
        n|N) : > "$SELECTED_FILE" ;;
        *)
          case "$answer" in
            *[!0-9]*) ;;
            *)
              key=$(awk -F'|' -v n="$answer" 'NF { i++; if (i == n) { print $1; exit } }' "$COMPONENTS_FILE")
              if [ -n "$key" ]; then
                if is_selected "$key"; then
                  unselect_one "$key"
                else
                  select_one "$key"
                fi
              fi
              ;;
          esac
          ;;
      esac
    done
  fi
fi

count=$(selected_count)

# A reinstall replaces only program files and the selected component folders.
# Ask once, and only when something would actually be replaced: a first install
# goes straight through without any extra confirmation.
replacing=0
if [ -f "$HOME_DIR/bin/install-manifest.txt" ]; then
  replacing=1
else
  while IFS= read -r key; do
    [ -n "$key" ] || continue
    if [ -d "$HOME_DIR/$(component_path "$key")" ]; then
      replacing=1
      break
    fi
  done < "$SELECTED_FILE"
fi
if [ "$replacing" -eq 1 ] && [ "$ASSUME_YES" -eq 0 ]; then
  printf '\n%s 已有心舰，将替换程序文件和已选组件目录，配置、会话、工作区保留。继续？[Y/n] ' "$HOME_DIR"
  if ! IFS= read -r reply; then
    reply=y
  fi
  case "$reply" in
    n|N) echo "已取消。"; exit 0 ;;
  esac
fi

printf '\n正在安装…\n'
mkdir -p "$HOME_DIR/bin" "$HOME_DIR/opt"

# Replacing a component means emptying its own folder only; config, sessions,
# workspaces, logs and every unselected component stay untouched.
while IFS= read -r key; do
  [ -n "$key" ] || continue
  path=$(component_path "$key")
  if [ -d "$HOME_DIR/$path" ]; then
    rm -rf -- "$HOME_DIR/$path"
  fi
done < "$SELECTED_FILE"

cp -f -- "$PAYLOAD_DIR/wunder-cli" "$HOME_DIR/bin/wunder-cli"
chmod 0755 "$HOME_DIR/bin/wunder-cli"

# The desktop payload is a plain tree archive of the AppDir. Expanding it at
# install time needs no FUSE, no /dev/fuse and no AppImage runtime, so
# fuse3-only systems (Ubuntu 24.04 and newer) work without extra packages.
rm -rf -- "$HOME_DIR/app/desktop"
mkdir -p "$HOME_DIR/app/desktop"
if ! tar -xzf "$PAYLOAD_DIR/wunder-desktop.tar.gz" -C "$HOME_DIR/app/desktop" --no-same-owner; then
  echo "蛹的程序文件解压失败。请检查磁盘空间后重新运行安装包。" >&2
  exit 1
fi
[ -x "$HOME_DIR/app/desktop/AppRun" ] || die "安装包缺少蛹的启动入口。"
rm -f -- "$HOME_DIR/bin/wunder-desktop.AppImage"
cat > "$HOME_DIR/bin/wunder-desktop" <<'LAUNCHER'
#!/bin/sh
DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$DIR/.." && pwd)
if [ -z "${WUNDER_HOME:-}" ]; then
  WUNDER_HOME="$ROOT"
  export WUNDER_HOME
fi
exec "$ROOT/app/desktop/AppRun" "$@"
LAUNCHER
chmod 0755 "$HOME_DIR/bin/wunder-desktop"
ln -sf -- "wunder-cli" "$HOME_DIR/bin/wunder"
ln -sf -- "wunder-desktop" "$HOME_DIR/bin/desktop"

LOCAL_BIN="$HOME/.local/bin"
LINKS_READY=1
link_command() {
  name=$1
  target=$2
  link="$LOCAL_BIN/$name"
  if [ -L "$link" ]; then
    current=$(readlink -- "$link" 2>/dev/null || true)
    if [ "$current" != "$target" ]; then
      ln -sf -- "$target" "$link" || LINKS_READY=0
    fi
  elif [ -e "$link" ]; then
    echo "提示：$link 已存在且不是符号链接，未覆盖；请自行将 $target 加入 PATH。" >&2
    LINKS_READY=0
  else
    ln -s -- "$target" "$link" || LINKS_READY=0
  fi
}

ensure_local_bin_path() {
  profile="$HOME/.profile"
  if [ -f "$profile" ] && grep -Fq '.local/bin' "$profile"; then
    return 0
  fi
  if [ ! -e "$profile" ]; then
    : > "$profile" || return 0
  fi
  cat >> "$profile" <<'PROFILE'

# >>> wunder commands >>>
case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *) PATH="$HOME/.local/bin:$PATH" ;;
esac
export PATH
# <<< wunder commands <<<
PROFILE
}

if mkdir -p "$LOCAL_BIN" 2>/dev/null; then
  link_command wunder-cli "$HOME_DIR/bin/wunder-cli"
  link_command wunder-desktop "$HOME_DIR/bin/wunder-desktop"
  link_command desktop "$HOME_DIR/bin/wunder-desktop"
  if [ "$LINKS_READY" -eq 1 ] && ! ensure_local_bin_path; then
    LINKS_READY=0
    echo "提示：请将 $LOCAL_BIN 加入 PATH 后重新打开终端。" >&2
  fi
else
  LINKS_READY=0
  echo "提示：无法创建 $LOCAL_BIN；请把 $HOME_DIR/bin 加入 PATH。" >&2
fi

if [ -f "$PAYLOAD_DIR/pupa.png" ]; then
  mkdir -p "$HOME_DIR/images"
  cp -f -- "$PAYLOAD_DIR/pupa.png" "$HOME_DIR/images/pupa.png"
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

previous_manifest="$HOME_DIR/bin/install-manifest.txt"
previous_desktop_entry=""
previous_desktop_icon=""
if [ -f "$previous_manifest" ]; then
  previous_desktop_entry=$(sed -n 's/^desktop_entry=//p' "$previous_manifest" | tail -n 1)
  previous_desktop_icon=$(sed -n 's/^desktop_icon=//p' "$previous_manifest" | tail -n 1)
fi
for previous in "$previous_desktop_entry" "$previous_desktop_icon"; do
  [ -n "$previous" ] || continue
  if [ -f "$previous" ] && grep -Fq 'X-Wunder-Install=1' "$previous"; then
    rm -f -- "$previous"
  fi
done

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

desktop_entry_path=""
desktop_icon_path=""
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
if [ "$DESKTOP_ENTRY" -eq 1 ] || [ "$DESKTOP_ICON" -eq 1 ]; then
  escaped_exec=$(printf '%s' "$HOME_DIR/bin/wunder-desktop" | sed 's/\\/\\\\/g; s/"/\\"/g')
  icon="$HOME_DIR/images/pupa.png"
  [ -f "$icon" ] || icon="$HOME_DIR/images/wunder.png"
  [ -f "$icon" ] || icon="wunder"

  if [ "$DESKTOP_ENTRY" -eq 1 ]; then
    entry_dir="$data_home/applications"
    desktop_entry_path="$entry_dir/wunder.desktop"
    if ! write_wunder_desktop_file "$desktop_entry_path" 0644; then
      echo "提示：无法创建应用菜单项：$desktop_entry_path" >&2
      desktop_entry_path=""
    fi
  fi

  if [ "$DESKTOP_ICON" -eq 1 ]; then
    desktop_dir=$(resolve_desktop_dir)
    desktop_icon_path="$desktop_dir/wunder.desktop"
    if write_wunder_desktop_file "$desktop_icon_path" 0755; then
      if command -v gio >/dev/null 2>&1; then
        gio set "$desktop_icon_path" metadata::trusted true >/dev/null 2>&1 || true
      fi
    else
      echo "提示：无法在桌面创建启动图标：$desktop_icon_path" >&2
      desktop_icon_path=""
    fi
  fi
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$data_home/applications" >/dev/null 2>&1 || true
fi
printf 'desktop_entry=%s\n' "$desktop_entry_path" >> "$HOME_DIR/bin/install-manifest.txt"
printf 'desktop_icon=%s\n' "$desktop_icon_path" >> "$HOME_DIR/bin/install-manifest.txt"

cp -f -- "$PAYLOAD_DIR/uninstall.sh" "$HOME_DIR/uninstall.sh"
chmod 0755 "$HOME_DIR/uninstall.sh"

printf '\n完成。\n\n'
printf '  蛹     %s\n' "$HOME_DIR/bin/wunder-desktop"
printf '  舵机   %s\n' "$HOME_DIR/bin/wunder-cli"
if [ "$count" -gt 0 ]; then
  printf '  组件   %s\n' "$components_csv"
else
  printf '  组件   未安装，重新运行安装包可勾选\n'
fi
if [ "$LINKS_READY" -eq 1 ]; then
  printf '  命令   wunder-desktop、wunder-cli、desktop（新开终端生效）\n'
else
  printf '  命令   %s/bin 未加入 PATH\n' "$HOME_DIR"
fi
if [ -n "$desktop_entry_path" ] || [ -n "$desktop_icon_path" ]; then
  entries=""
  if [ -n "$desktop_entry_path" ]; then
    entries="应用菜单"
  fi
  if [ -n "$desktop_icon_path" ]; then
    entries="${entries:+$entries + }桌面图标"
  fi
  printf '  入口   %s已创建\n' "$entries"
fi
printf '  卸载   %s\n' "$HOME_DIR/uninstall.sh"
