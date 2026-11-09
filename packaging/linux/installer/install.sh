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
用法: wunder-desktop-<版本>-linux-<架构>.run [选项]

直接运行即可，安装过程没有任何提问：桌面版、命令行与全部补充组件一起装到
默认安装目录，并创建应用菜单项和桌面启动图标。检测到已有安装时原地升级：
只替换程序文件与组件目录，配置、会话、工作区保留。

  --home=PATH          改安装目录（配置、会话、工作区也放在这里），默认 ~/.wunder
      --components=LIST 高级用法：只安装列出的补充组件，逗号分隔，例如 python,git
      --no-desktop-entry   不创建应用菜单项
      --no-desktop-icon    不在桌面创建启动图标
      --no-progress        不显示进度条（设置 NO_COLOR 或重定向到文件时自动关闭）
  -h, --help           显示帮助
EOF
}

write_wunder_desktop_file() {
  target=$1
  mode=$2
  mkdir -p "$(dirname -- "$target")" || return 1
  cat > "$target" <<EOF
[Desktop Entry]
Type=Application
Name=wunder 桌面版
Comment=本地 AI 工作台
StartupWMClass=wunder
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

component_bytes() {
  awk -F'|' -v key="$1" '$1 == key { print $5; exit }' "$COMPONENTS_FILE"
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

# ---- 进度显示 -------------------------------------------------------------
# .run 头部用 WUNDER_PROGRESS_* 传入进度区间（默认 55 → 100），这里只负责在
# 自己的区间里推进。进度写 stderr，stdout 留给最终摘要。
PROGRESS_BASE=${WUNDER_PROGRESS_BASE:-0}
PROGRESS_WEIGHT=${WUNDER_PROGRESS_WEIGHT:-100}
PROGRESS_LIVE=${WUNDER_PROGRESS_LIVE:-0}
PROGRESS_ON=${WUNDER_PROGRESS:-1}
if [ "$PROGRESS_ON" -eq 1 ] && [ -t 2 ]; then
  PROGRESS_LIVE=1
else
  PROGRESS_LIVE=0
fi
PROGRESS_LAST_TICK=""
PROGRESS_OPEN=0

progress_bar() {
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

progress_draw() {
  [ "$PROGRESS_ON" -eq 1 ] || return 0
  percent=$1
  label=$2
  [ "$percent" -gt 100 ] && percent=100
  [ "$percent" -lt 0 ] && percent=0
  if [ "$PROGRESS_LIVE" -eq 1 ]; then
    printf '\r\033[K  [%s] %3d%%  %s' "$(progress_bar 24 "$percent")" "$percent" "$label" >&2
    PROGRESS_OPEN=1
  else
    tick=$((percent / 10))
    if [ "$tick" != "$PROGRESS_LAST_TICK" ]; then
      PROGRESS_LAST_TICK=$tick
      printf '  [%s] %3d%%  %s\n' "$(progress_bar 24 "$percent")" "$percent" "$label" >&2
    fi
  fi
}

progress_stage() {
  offset=$1
  label=$2
  progress_draw $((PROGRESS_BASE + offset * PROGRESS_WEIGHT / 100)) "$label"
}

progress_done() {
  if [ "$PROGRESS_OPEN" -eq 1 ]; then
    # 光标离开进度行：先清掉这一行，再换行，后续摘要从行首开始。
    printf '\r\033[K\n' >&2
    PROGRESS_OPEN=0
  fi
}

# 按当前阶段已落盘字节推进；du 不可用时静默跳过，不影响安装。
progress_watch() {
  offset=$1
  label=$2
  total=$3
  target=$4
  while [ ! -f "$target" ]; do
    used=$(du -sb --apparent-size "$5" 2>/dev/null | cut -f1)
    used=${used:-0}
    [ "$used" -gt "$total" ] && used=$total
    if [ "$total" -gt 0 ]; then
      progress_stage $((offset + used * (100 - offset) / total)) "$label"
    fi
    sleep 1
  done
}

select_one() {
  grep -Fxq -- "$1" "$SELECTED_FILE" 2>/dev/null || printf '%s\n' "$1" >> "$SELECTED_FILE"
}

selected_count() {
  grep -c . "$SELECTED_FILE" 2>/dev/null || true
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    # 历史参数：安装已无交互、默认全选，保留以便旧脚本继续可用。
    -y|--yes|--all) ;;
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
    # 进度条由 .run 头部控制，这里只负责让参数不报错。
    --progress|--no-progress) ;;
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

# 运行即装全：载荷里带的就是完整工作站，安装过程不再提问、不再让人挑组件。
select_all_components() {
  cut -d'|' -f1 "$COMPONENTS_FILE" | while IFS= read -r key; do
    [ -n "$key" ] && printf '%s\n' "$key"
  done
}

if [ "$COMPONENTS_GIVEN" -eq 1 ]; then
  : > "$SELECTED_FILE"
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
else
  select_all_components > "$SELECTED_FILE"
fi

count=$(selected_count)

# 补充组件总量：用于进度百分比与“清理旧文件”的提示。
total_components=0
while IFS= read -r key; do
  [ -n "$key" ] || continue
  bytes=$(component_bytes "$key")
  case "$bytes" in
    ''|*[!0-9]*) bytes=0 ;;
  esac
  total_components=$((total_components + bytes))
done < "$SELECTED_FILE"
total_cleanup=$total_components
for existing in "$HOME_DIR/app/desktop" "$HOME_DIR/bin/wunder-cli"; do
  [ -e "$existing" ] || continue
  size=$(du -sb --apparent-size "$existing" 2>/dev/null | cut -f1)
  total_cleanup=$((total_cleanup + ${size:-0}))
done

# 已有安装直接原地升级：只替换程序文件与本次组件目录，用户数据不参与。
printf '\n心舰 Wunder %s · %s\n安装到 %s\n' "$VERSION" "$ARCH" "$HOME_DIR"
if [ -f "$HOME_DIR/bin/install-manifest.txt" ]; then
  printf '检测到已有安装，升级程序文件与组件目录（配置、会话、工作区保留）。\n'
fi
mkdir -p "$HOME_DIR/bin" "$HOME_DIR/opt"

# 阶段划分（占安装区间百分比）：清理 0-3，命令行 3-10，桌面版 10-40，
# 命令链接与图标 40-45，补充组件 45-100。
progress_stage 0 "清理旧文件（$(human_size "$total_cleanup")）"

# Replacing a component means emptying its own folder only; config, sessions,
# workspaces, logs and every unselected component stay untouched.
while IFS= read -r key; do
  [ -n "$key" ] || continue
  path=$(component_path "$key")
  if [ -d "$HOME_DIR/$path" ]; then
    rm -rf -- "$HOME_DIR/$path"
  fi
done < "$SELECTED_FILE"
progress_stage 3 "清理旧文件（$(human_size "$total_cleanup")）"

cp -f -- "$PAYLOAD_DIR/wunder-cli" "$HOME_DIR/bin/wunder-cli"
chmod 0755 "$HOME_DIR/bin/wunder-cli"
progress_stage 10 "安装命令行"

# The desktop payload is a plain tree archive of the AppDir. Expanding it at
# install time needs no FUSE, no /dev/fuse and no AppImage runtime, so
# fuse3-only systems (Ubuntu 24.04 and newer) work without extra packages.
rm -rf -- "$HOME_DIR/app/desktop"
mkdir -p "$HOME_DIR/app/desktop"
desktop_bytes=$(wc -c < "$PAYLOAD_DIR/wunder-desktop.tar.gz" | tr -d ' ')
desktop_bytes=${desktop_bytes:-0}
if [ "$PROGRESS_ON" -eq 1 ]; then
  progress_watch 10 "安装桌面版（$(human_size "$desktop_bytes")）" "$desktop_bytes" \
    "$PAYLOAD_DIR/../.desktop-done" "$HOME_DIR/app/desktop" &
  DESKTOP_WATCH_PID=$!
fi
if ! tar -xzf "$PAYLOAD_DIR/wunder-desktop.tar.gz" -C "$HOME_DIR/app/desktop" --no-same-owner; then
  [ -n "${DESKTOP_WATCH_PID:-}" ] && kill "$DESKTOP_WATCH_PID" 2>/dev/null || true
  progress_done
  echo "桌面版的程序文件解压失败。请检查磁盘空间后重新运行安装包。" >&2
  exit 1
fi
if [ -n "${DESKTOP_WATCH_PID:-}" ]; then
  : > "$PAYLOAD_DIR/../.desktop-done"
  wait "$DESKTOP_WATCH_PID" 2>/dev/null || true
  DESKTOP_WATCH_PID=""
fi
progress_stage 40 "安装桌面版（$(human_size "$desktop_bytes")）"
[ -x "$HOME_DIR/app/desktop/AppRun" ] || die "安装包缺少桌面版的启动入口。"
rm -f -- "$HOME_DIR/bin/wunder-desktop.AppImage"
# Upgrades from releases that shipped the old executable name must not leave a
# stale launcher beside the new one.
rm -f -- "$HOME_DIR/bin/wunder-frontend-slint"
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

if [ -f "$PAYLOAD_DIR/eva01-head.png" ]; then
  mkdir -p "$HOME_DIR/images"
  cp -f -- "$PAYLOAD_DIR/eva01-head.png" "$HOME_DIR/images/eva01-head.png"
  # Older installers wrote the pupa image under this name; drop it on upgrade.
  rm -f -- "$HOME_DIR/images/pupa.png"
fi
progress_stage 45 "配置命令与桌面入口"

if [ "$count" -gt 0 ]; then
  members=""
  while IFS= read -r key; do
    [ -n "$key" ] || continue
    path=$(component_path "$key")
    [ -n "$path" ] || die "组件清单缺少路径：$key"
    members="$members $path"
  done < "$SELECTED_FILE"
  progress_stage 45 "安装补充组件（$(human_size "$total_components")）"
  if [ "$PROGRESS_ON" -eq 1 ]; then
    progress_watch 45 "安装补充组件（$(human_size "$total_components")）" "$total_components" \
      "$PAYLOAD_DIR/../.supplement-done" "$HOME_DIR/opt" &
    SUPPLEMENT_WATCH_PID=$!
  fi
  if ! tar -xzf "$PAYLOAD_DIR/supplement.tar.gz" -C "$HOME_DIR" --no-same-owner $members; then
    [ -n "${SUPPLEMENT_WATCH_PID:-}" ] && kill "$SUPPLEMENT_WATCH_PID" 2>/dev/null || true
    progress_done
    echo "补充组件解压失败。请检查磁盘空间后重新运行安装包；再次安装会先清理本次留下的目录。" >&2
    exit 1
  fi
  if [ -n "${SUPPLEMENT_WATCH_PID:-}" ]; then
    : > "$PAYLOAD_DIR/../.supplement-done"
    wait "$SUPPLEMENT_WATCH_PID" 2>/dev/null || true
    SUPPLEMENT_WATCH_PID=""
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
  icon="$HOME_DIR/images/eva01-head.png"
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

progress_stage 100 "安装完成"
progress_done

printf '\n完成。\n\n'
printf '  桌面版     %s\n' "$HOME_DIR/bin/wunder-desktop"
printf '  命令行   %s\n' "$HOME_DIR/bin/wunder-cli"
if [ "$count" -gt 0 ]; then
  printf '  组件   %s\n' "$components_csv"
else
  printf '  组件   未安装（--components 为空）\n'
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
