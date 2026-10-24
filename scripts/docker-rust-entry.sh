#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"
binary="${CARGO_TARGET_DIR:-/tmp/cargo-target}/release/wunder-server"
prefer_prebuilt="${WUNDER_PREFER_PREBUILT_BIN:-0}"
feature_stamp="${binary}.features"

normalize_server_features() {
  local raw="${1:-}"
  local normalized=""
  local part

  raw="${raw//,/ }"
  for part in ${raw}; do
    if [ -z "${part}" ]; then
      continue
    fi
    if [ -n "${normalized}" ]; then
      normalized="${normalized} ${part}"
    else
      normalized="${part}"
    fi
  done

  printf '%s' "${normalized}"
}

server_features="$(normalize_server_features "${WUNDER_SERVER_FEATURES:-mcp,host-metrics,web-fetch}")"

ensure_playwright_browsers() {
  local target="${PLAYWRIGHT_BROWSERS_PATH:-}"
  local seed="${WUNDER_PLAYWRIGHT_SEED_PATH:-/opt/ms-playwright}"

  if [ -z "${target}" ] || [ ! -d "${seed}" ]; then
    return 0
  fi

  mkdir -p "${target}"
  if find "${target}" -maxdepth 1 -type d -name 'chromium*' -print -quit 2>/dev/null | grep -q .; then
    return 0
  fi
  if ! find "${seed}" -maxdepth 1 -type d -name 'chromium*' -print -quit 2>/dev/null | grep -q .; then
    return 0
  fi

  printf '%s\n' "[docker][browser] seeding Playwright Chromium into ${target}" >&2
  cp -a "${seed}/." "${target}/"
}

binary_is_ready() {
  if [ ! -x "${binary}" ]; then
    return 1
  fi

  if [ "${prefer_prebuilt}" = "1" ]; then
    return 0
  fi

  if [ ! -f "${feature_stamp}" ] || [ "$(cat "${feature_stamp}" 2>/dev/null || true)" != "${server_features}" ]; then
    return 1
  fi

  ! find Cargo.toml Cargo.lock crates/wunder-core crates/wunder-runtime crates/wunder-server scripts/docker-rust-entry.sh \
    -type f -newer "${binary}" -print -quit 2>/dev/null | grep -q .
}

# 视频制作等技能需要的 Node/Remotion 运行时由 config/runtime 挂载提供，
# 启动时解压到容器内固定路径；升级运行时只需替换运行时包并重启容器，不必重建镜像。
ensure_runtime_bundle() {
  local source="${WUNDER_RUNTIME_SOURCE:-/run/wunder-runtime}"
  local target="${WUNDER_RUNTIME_DIR:-/opt/wunder-runtime}"

  [ -d "${source}" ] || return 0

  local arch sub pkg_dir
  arch="$(uname -m)"
  case "${arch}" in
    x86_64 | amd64) sub=linux-x64 ;;
    aarch64 | arm64) sub=linux-arm64 ;;
    *) return 0 ;;
  esac

  pkg_dir="${source}/${sub}"
  [ -d "${pkg_dir}" ] || return 0
  if ! find "${pkg_dir}" -maxdepth 1 -name '*.tar.gz' -print -quit 2>/dev/null | grep -q .; then
    return 0
  fi

  local stamp="${target}/.stamp"
  local want
  want="$(cd "${pkg_dir}" &&
    for bundle in *.tar.gz; do
      printf '%s:%s\n' "${bundle}" "$(stat -c '%s' "${bundle}" 2>/dev/null || echo 0)"
    done | sha256sum | cut -d' ' -f1)"

  if [ -f "${stamp}" ] && [ "$(cat "${stamp}" 2>/dev/null || true)" = "${want}" ] &&
    [ -d "${target}/remotion/node_modules" ]; then
    return 0
  fi

  printf '%s\n' "[docker][runtime] unpacking ${pkg_dir} -> ${target}" >&2
  rm -rf "${target}"
  if ! mkdir -p "${target}"; then
    printf '%s\n' "[docker][runtime] ${target} is not writable (read_only container?), skipping" >&2
    return 0
  fi

  local bundle
  for bundle in "${pkg_dir}"/*.tar.gz; do
    tar -xzf "${bundle}" -C "${target}" ||
      printf '%s\n' "[docker][runtime] failed to unpack ${bundle}" >&2
  done
  printf '%s' "${want}" > "${stamp}"
}

# Linux 上的执行命令走 `bash -lc`，登录 shell 会重置 PATH，
# 所以运行时入口必须通过 profile.d 注册，不能只靠容器 ENV。
ensure_runtime_profile() {
  local target="${WUNDER_RUNTIME_DIR:-/opt/wunder-runtime}"
  [ -d "${target}/remotion/node_modules" ] || return 0

  local profile=/etc/profile.d/wunder-runtime.sh
  if ! {
    printf '%s\n' \
      "export WUNDER_RUNTIME_ROOT=${target}" \
      "export WUNDER_REMOTION_ROOT=${target}/remotion" \
      'case ":${PATH}:" in *":${WUNDER_RUNTIME_ROOT}/remotion/bin:"*) ;; *) export PATH="${WUNDER_RUNTIME_ROOT}/remotion/bin:${WUNDER_RUNTIME_ROOT}/node/bin:${PATH}" ;; esac' \
      'export NODE_PATH="${WUNDER_REMOTION_ROOT}/node_modules:${NODE_PATH:-/usr/lib/node_modules:/usr/local/lib/node_modules}"'
  } > "${profile}" 2>/dev/null; then
    printf '%s\n' "[docker][runtime] cannot write ${profile}; login shells will miss the runtime PATH" >&2
  fi
}

# 运行时包同时带了 npm 离线缓存，解包到可写卷后 `npm install --offline` 才能装
# 预取过的依赖；缓存是内容寻址的，与用户已有缓存合并不会互相破坏。
ensure_npm_cache_bundle() {
  local source="${WUNDER_RUNTIME_SOURCE:-/run/wunder-runtime}"
  local target="${NPM_CONFIG_CACHE:-/workspaces/.cache/npm}"

  local arch sub pkg_dir bundle
  arch="$(uname -m)"
  case "${arch}" in
    x86_64 | amd64) sub=linux-x64 ;;
    aarch64 | arm64) sub=linux-arm64 ;;
    *) return 0 ;;
  esac

  pkg_dir="${source}/${sub}"
  bundle="${pkg_dir}/npm-cache.tar.gz"
  [ -f "${bundle}" ] || return 0

  local stamp="${target}/.wunder-seed"
  local want
  want="$(stat -c '%s' "${bundle}" 2>/dev/null || echo 0)"

  if [ -f "${stamp}" ] && [ "$(cat "${stamp}" 2>/dev/null || true)" = "${want}" ]; then
    return 0
  fi

  printf '%s\n' "[docker][runtime] unpacking npm offline cache -> ${target}" >&2
  if ! mkdir -p "${target}"; then
    printf '%s\n' "[docker][runtime] ${target} is not writable, skipping npm cache" >&2
    return 0
  fi
  tar -xzf "${bundle}" -C "${target}" ||
    printf '%s\n' "[docker][runtime] failed to unpack ${bundle}" >&2
  printf '%s' "${want}" > "${stamp}"
}

# 视频渲染与依赖安装需要可写临时目录：sandbox 容器的 /tmp 是内存 tmpfs，
# 中间帧、npm 缓存和渲染临时文件应落在挂载卷上，避免写满内存。
ensure_scratch_dirs() {
  local dir
  for dir in "${TMPDIR:-}" "${NPM_CONFIG_CACHE:-}"; do
    if [ -n "${dir}" ]; then
      mkdir -p "${dir}" 2>/dev/null ||
        printf '%s\n' "[docker][runtime] cannot create ${dir}" >&2
    fi
  done
}

# Cargo/rustc create temporary directories during dependency compilation.
# This must run before the first binary check/build: source-mounted compose
# deployments map /workspaces from the host, where .cache/tmp may not exist
# yet.  Keeping it in the shared entrypoint also covers the sandbox service.
ensure_scratch_dirs

run_binary() {
  ensure_runtime_bundle
  ensure_runtime_profile
  ensure_npm_cache_bundle
  ensure_scratch_dirs
  ensure_playwright_browsers
  exec "${binary}"
}

if [ -x "${binary}" ] && [ "${prefer_prebuilt}" = "1" ]; then
  printf '%s\n' "[docker][rust] reusing prebuilt release binary because WUNDER_PREFER_PREBUILT_BIN=1" >&2
fi

case "${mode}" in
  wait-or-run)
    if binary_is_ready; then
      run_binary
    fi

    printf '%s\n' "wunder-server missing or stale, waiting for shared build output..." >&2
    while true; do
      if binary_is_ready; then
        run_binary
      fi
      sleep 1
    done
    ;;
  run-or-build)
    if binary_is_ready; then
      run_binary
    fi

    # A source-mounted compose deployment must be able to resolve dependencies
    # introduced by a newer Cargo.lock. Cargo uses its cache first and fetches
    # only a missing package; forcing --offline here made a normal update loop
    # forever when the mounted cache did not yet contain that package.
    CARGO_NET_OFFLINE=false cargo build --locked --release -p wunder-server --bin wunder-server --features "${server_features}"
    printf '%s' "${server_features}" > "${feature_stamp}"
    run_binary
    ;;
  *)
    printf 'Usage: %s <wait-or-run|run-or-build>\n' "$0" >&2
    exit 64
    ;;
esac
