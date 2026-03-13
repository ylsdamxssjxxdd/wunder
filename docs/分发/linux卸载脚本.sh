#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUNDLE_DIR="${WUNDER_BUNDLE_DIR:-$SCRIPT_DIR}"
DRY_RUN=0
ASSUME_YES=0
REMOVE_SYSTEM=0
REMOVE_BUNDLE_APPIMAGE=1
EXTRA_PATHS=()

usage() {
  cat <<'EOF'
Wunder Linux 卸载脚本

用途：
  - 删除 Wunder Desktop 本体、桌面集成、用户数据、缓存与常见安装残留
  - 默认保留 wunder补充包 / wunder-extra / wunder-python 及其压缩包

用法：
  ./linux卸载脚本.sh [options]

选项：
  --yes               免确认直接执行
  --dry-run           仅打印将删除的内容，不实际删除
  --system            额外清理系统级安装路径（可能需要 sudo）
  --bundle-dir DIR    指定 AppImage 所在目录，默认是脚本所在目录
  --extra-path PATH   追加一个要删除的自定义路径，可重复传入
  --no-bundle-appimage  不删除分发目录中的 wunder-desktop*.AppImage
  -h, --help          显示帮助

说明：
  1) 本脚本不会删除 wunder补充包 / wunder-extra / wunder-python，也不会删除对应 tar.* 包。
  2) 如果你曾用自定义 --temp-root / --workspace 启动 Wunder，请用 --extra-path 追加这些目录。
EOF
}

log() {
  printf '[wunder-uninstall] %s\n' "$*"
}

warn() {
  printf '[wunder-uninstall][warn] %s\n' "$*" >&2
}

run_cmd() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '[dry-run]'
    for arg in "$@"; do
      printf ' %q' "$arg"
    done
    printf '\n'
    return 0
  fi
  "$@"
}

run_system_cmd() {
  if [ "$EUID" -eq 0 ]; then
    run_cmd "$@"
    return 0
  fi
  if command -v sudo >/dev/null 2>&1; then
    if [ "$DRY_RUN" -eq 1 ]; then
      printf '[dry-run] sudo'
      for arg in "$@"; do
        printf ' %q' "$arg"
      done
      printf '\n'
      return 0
    fi
    sudo "$@"
    return 0
  fi
  warn "system scope cleanup requires root or sudo: $*"
  return 1
}

contains_path() {
  local target="$1"
  shift || true
  local item
  for item in "$@"; do
    if [ "$item" = "$target" ]; then
      return 0
    fi
  done
  return 1
}

append_path() {
  local path="$1"
  local -n bucket_ref="$2"
  [ -n "$path" ] || return 0
  if contains_path "$path" "${bucket_ref[@]:-}"; then
    return 0
  fi
  bucket_ref+=("$path")
}

append_existing_path() {
  local path="$1"
  if [ -e "$path" ] || [ -L "$path" ]; then
    append_path "$path" "$2"
  fi
}

append_glob_matches() {
  local bucket_name="$1"
  local -n bucket_ref="$bucket_name"
  shift
  local pattern=""
  local matched=""
  for pattern in "$@"; do
    while IFS= read -r matched; do
      [ -n "$matched" ] || continue
      append_path "$matched" "$bucket_name"
    done < <(compgen -G "$pattern" || true)
  done
}

stop_matching_processes() {
  # Best-effort shutdown before deleting files to avoid leaving file locks or zombie tray entries.
  local pattern='(^|/)(wunder-desktop|wunder-desktop-bridge)([[:space:]]|$)'
  if command -v pkill >/dev/null 2>&1; then
    if [ "$DRY_RUN" -eq 1 ]; then
      log "would stop running wunder-desktop processes"
    else
      pkill -f "$pattern" >/dev/null 2>&1 || true
    fi
  fi
}

stop_user_units() {
  local unit_path=""
  local unit_name=""
  if ! command -v systemctl >/dev/null 2>&1; then
    return 0
  fi
  for unit_path in "$@"; do
    [ -n "$unit_path" ] || continue
    unit_name="$(basename "$unit_path")"
    if [ "$DRY_RUN" -eq 1 ]; then
      log "would stop user unit ${unit_name}"
      continue
    fi
    systemctl --user disable --now "$unit_name" >/dev/null 2>&1 || true
  done
}

stop_system_units() {
  local unit_path=""
  local unit_name=""
  if ! command -v systemctl >/dev/null 2>&1; then
    return 0
  fi
  for unit_path in "$@"; do
    [ -n "$unit_path" ] || continue
    unit_name="$(basename "$unit_path")"
    run_system_cmd systemctl disable --now "$unit_name" >/dev/null 2>&1 || true
  done
}

remove_paths() {
  local scope="$1"
  shift
  local path=""
  for path in "$@"; do
    [ -n "$path" ] || continue
    if [ ! -e "$path" ] && [ ! -L "$path" ]; then
      continue
    fi
    log "removing ${path}"
    if [ "$scope" = 'system' ]; then
      run_system_cmd rm -rf -- "$path"
    else
      run_cmd rm -rf -- "$path"
    fi
  done
}

daemon_reload_if_needed() {
  if ! command -v systemctl >/dev/null 2>&1; then
    return 0
  fi
  if [ "$DRY_RUN" -eq 1 ]; then
    log 'would reload systemd user daemon'
  else
    systemctl --user daemon-reload >/dev/null 2>&1 || true
  fi
  if [ "$REMOVE_SYSTEM" -eq 1 ]; then
    run_system_cmd systemctl daemon-reload >/dev/null 2>&1 || true
  fi
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --yes)
      ASSUME_YES=1
      ;;
    --dry-run)
      DRY_RUN=1
      ;;
    --system)
      REMOVE_SYSTEM=1
      ;;
    --bundle-dir)
      shift
      [ "$#" -gt 0 ] || { warn '--bundle-dir requires a directory'; exit 1; }
      BUNDLE_DIR="$1"
      ;;
    --extra-path)
      shift
      [ "$#" -gt 0 ] || { warn '--extra-path requires a path'; exit 1; }
      EXTRA_PATHS+=("$1")
      ;;
    --no-bundle-appimage)
      REMOVE_BUNDLE_APPIMAGE=0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      warn "unknown option: $1"
      usage >&2
      exit 1
      ;;
  esac
  shift
done

USER_PATHS=()
USER_UNIT_FILES=()
SYSTEM_PATHS=()
SYSTEM_UNIT_FILES=()
PRESERVED_PATHS=()

# User-scoped runtime data created by Wunder Desktop.
append_existing_path "$HOME/.config/Wunder Desktop" USER_PATHS
append_existing_path "$HOME/.cache/Wunder Desktop" USER_PATHS
append_existing_path "$HOME/.local/share/Wunder Desktop" USER_PATHS
append_existing_path "$HOME/.local/state/Wunder Desktop" USER_PATHS
append_existing_path "$HOME/.config/wunder-desktop" USER_PATHS
append_existing_path "$HOME/.cache/wunder-desktop" USER_PATHS
append_existing_path "$HOME/.local/share/wunder-desktop" USER_PATHS
append_existing_path "$HOME/.local/state/wunder-desktop" USER_PATHS
append_existing_path "$HOME/.config/wunder" USER_PATHS
append_existing_path "$HOME/.cache/wunder" USER_PATHS
append_existing_path "$HOME/.local/share/wunder" USER_PATHS
append_existing_path "$HOME/.local/state/wunder" USER_PATHS
append_existing_path "$HOME/.local/bin/wunder-desktop" USER_PATHS
append_existing_path "$HOME/.local/bin/wunder-desktop-bridge" USER_PATHS

# Honor explicit runtime overrides when the caller knows custom locations.
if [ -n "${WUNDER_TEMPD:-}" ]; then
  append_existing_path "$WUNDER_TEMPD" USER_PATHS
fi
if [ -n "${WUNDER_WORK:-}" ]; then
  append_existing_path "$WUNDER_WORK" USER_PATHS
fi

append_glob_matches USER_PATHS \
  "$HOME/.local/share/applications/*wunder*.desktop" \
  "$HOME/.config/autostart/*wunder*.desktop" \
  "$HOME/.local/share/icons/hicolor/*/apps/*wunder*.*" \
  "$HOME/.local/share/metainfo/*wunder*.*" \
  "$HOME/.config/menus/applications-merged/*wunder*.*"

append_glob_matches USER_UNIT_FILES \
  "$HOME/.config/systemd/user/*wunder*.service"

if [ "$REMOVE_BUNDLE_APPIMAGE" -eq 1 ]; then
  append_glob_matches USER_PATHS \
    "$BUNDLE_DIR/wunder-desktop*.AppImage" \
    "$BUNDLE_DIR/wunder-desktop*.zsync"
fi

# Preserve sidecar bundles built from packaging/docker.
append_glob_matches PRESERVED_PATHS \
  "$BUNDLE_DIR/wunder补充包*" \
  "$BUNDLE_DIR/wunder-extra*" \
  "$BUNDLE_DIR/wunder-python*" \
  "$BUNDLE_DIR/wunder补充包-*.tar.*" \
  "$BUNDLE_DIR/wunder-extra-*.tar.*" \
  "$BUNDLE_DIR/wunder-python-*.tar.*"

for path in "${EXTRA_PATHS[@]}"; do
  if [ -e "$path" ] || [ -L "$path" ]; then
    if [ -w "$path" ] || [ -w "$(dirname "$path")" ]; then
      append_existing_path "$path" USER_PATHS
    else
      append_existing_path "$path" SYSTEM_PATHS
    fi
  fi
done

if [ "$REMOVE_SYSTEM" -eq 1 ]; then
  append_existing_path "/opt/wunder-desktop" SYSTEM_PATHS
  append_existing_path "/opt/Wunder Desktop" SYSTEM_PATHS
  append_existing_path "/usr/local/bin/wunder-desktop" SYSTEM_PATHS
  append_existing_path "/usr/local/bin/wunder-desktop-bridge" SYSTEM_PATHS
  append_existing_path "/usr/bin/wunder-desktop" SYSTEM_PATHS
  append_existing_path "/usr/bin/wunder-desktop-bridge" SYSTEM_PATHS
  append_existing_path "/usr/share/Wunder Desktop" SYSTEM_PATHS
  append_existing_path "/usr/local/share/Wunder Desktop" SYSTEM_PATHS

  append_glob_matches SYSTEM_PATHS \
    "/usr/share/applications/*wunder*.desktop" \
    "/usr/local/share/applications/*wunder*.desktop" \
    "/usr/share/icons/hicolor/*/apps/*wunder*.*" \
    "/usr/local/share/icons/hicolor/*/apps/*wunder*.*" \
    "/usr/share/metainfo/*wunder*.*" \
    "/usr/local/share/metainfo/*wunder*.*"

  append_glob_matches SYSTEM_UNIT_FILES \
    "/etc/systemd/system/*wunder*.service" \
    "/usr/lib/systemd/system/*wunder*.service"
fi

if [ "${#USER_UNIT_FILES[@]}" -gt 0 ]; then
  stop_user_units "${USER_UNIT_FILES[@]}"
  for path in "${USER_UNIT_FILES[@]}"; do
    append_path "$path" USER_PATHS
  done
fi

if [ "$REMOVE_SYSTEM" -eq 1 ] && [ "${#SYSTEM_UNIT_FILES[@]}" -gt 0 ]; then
  stop_system_units "${SYSTEM_UNIT_FILES[@]}"
  for path in "${SYSTEM_UNIT_FILES[@]}"; do
    append_path "$path" SYSTEM_PATHS
  done
fi

stop_matching_processes

log 'the following paths will be removed:'
for path in "${USER_PATHS[@]}"; do
  printf '  [user]   %s\n' "$path"
done
for path in "${SYSTEM_PATHS[@]}"; do
  printf '  [system] %s\n' "$path"
done

if [ "${#USER_PATHS[@]}" -eq 0 ] && [ "${#SYSTEM_PATHS[@]}" -eq 0 ]; then
  log 'no Wunder paths were found'
fi

if [ "${#PRESERVED_PATHS[@]}" -gt 0 ]; then
  log 'the following sidecar bundles are preserved:'
  for path in "${PRESERVED_PATHS[@]}"; do
    printf '  [keep]   %s\n' "$path"
  done
fi

if [ "$ASSUME_YES" -ne 1 ]; then
  printf 'Continue uninstall? [y/N] '
  read -r answer
  case "${answer}" in
    y|Y|yes|YES)
      ;;
    *)
      log 'cancelled'
      exit 0
      ;;
  esac
fi

remove_paths user "${USER_PATHS[@]}"
if [ "$REMOVE_SYSTEM" -eq 1 ]; then
  remove_paths system "${SYSTEM_PATHS[@]}"
fi

daemon_reload_if_needed

log 'uninstall finished'
if [ "$REMOVE_BUNDLE_APPIMAGE" -eq 1 ]; then
  log 'sidecar bundles were intentionally kept'
fi