#!/usr/bin/env bash
# 服务版 Node/Remotion 运行时包生成器（在容器内执行，跨平台一致）。
# 由 scripts/prepare-node-runtime.sh 调用，参数全部通过环境变量传入。
set -euo pipefail

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# 缓存落在工作目录里，安装完成后随运行时一起打包，
# 让离线的项目在需要额外依赖时仍能用 `npm install --offline` 安装。
export NPM_CONFIG_CACHE="$WORK/npm-cache"

arch="$(uname -m)"
case "$arch" in
  x86_64 | amd64) node_arch=x64 ;;
  aarch64 | arm64) node_arch=arm64 ;;
  *)
    printf '%s\n' "unsupported arch: $arch" >&2
    exit 1
    ;;
esac

# ---------- Node ----------
node_dir="$WORK/node-v$NODE_VERSION-linux-$node_arch"
if [ ! -x "$node_dir/bin/node" ]; then
  printf '%s\n' "[runtime] downloading node $NODE_VERSION ($node_arch)"
  curl -fsSL "$NODE_BASE/v$NODE_VERSION/node-v$NODE_VERSION-linux-$node_arch.tar.gz" -o "$WORK/node.tar.gz"
  tar -xzf "$WORK/node.tar.gz" -C "$WORK"
fi

# 官方包的 bin/npm、bin/npx、bin/corepack 是符号链接；宿主卷可能不支持，
# 统一换成等价的 shell 脚本，保证解压到任意文件系统都可用。
rm -f "$node_dir/bin/npm" "$node_dir/bin/npx" "$node_dir/bin/corepack"
printf '%s\n' \
  '#!/bin/sh' \
  'here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)' \
  'exec "$here/node" "$here/../lib/node_modules/npm/bin/npm-cli.js" "$@"' \
  > "$node_dir/bin/npm"
printf '%s\n' \
  '#!/bin/sh' \
  'here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)' \
  'exec "$here/node" "$here/../lib/node_modules/npm/bin/npx-cli.js" "$@"' \
  > "$node_dir/bin/npx"
chmod +x "$node_dir/bin/npm" "$node_dir/bin/npx"

printf '%s\n' "[runtime] packing node"
tar -czf "$OUT_DIR/node.tar.gz" -C "$WORK" --transform 's,^node-v[^/]*,node,' "node-v$NODE_VERSION-linux-$node_arch"

# ---------- 真实项目的解析结果 ----------
# 项目执行 `npm install <pkg>` 时 npm 会重新解析 peer 依赖（例如 react 取当前 registry
# 上的最新 19.x），解析出的 tarball 若不在缓存里，离线安装就会 ENOTCACHED。
# 这里按项目的方式再解析一次，只保留 npm 缓存，不保留 node_modules。
printf '%s\n' "[runtime] resolving dependency graph the way a project would"
loose_dir="$WORK/loose"
mkdir -p "$loose_dir"
(
  cd "$loose_dir"
  printf '%s\n' '{"name":"wunder-project-graph","private":true}' > package.json
  # react/react-dom 故意不带版本：项目安装时 npm 会挑 registry 上的最新 19.x，
  # 只有缓存里也有这份 tarball，离线安装才不会缺包。
  npm install --no-bin-links --no-audit --no-fund \
    react \
    react-dom \
    ${EXTRA_DEPS:-} >/dev/null 2>&1 ||
    printf '%s\n' "[runtime] warn: project-graph resolution failed, cache may be partial" >&2
)
rm -rf "$loose_dir"

# ---------- Remotion ----------
remotion_dir="$WORK/remotion"
mkdir -p "$remotion_dir"
printf '%s\n' '{"name":"wunder-remotion-runtime","private":true}' > "$remotion_dir/package.json"
(
  cd "$remotion_dir"
  printf '%s\n' "[runtime] installing remotion $REMOTION_VERSION"
  npm install --no-bin-links --no-audit --no-fund \
    "remotion@$REMOTION_VERSION" \
    "@remotion/cli@$REMOTION_VERSION" \
    "react@$REACT_VERSION" \
    "react-dom@$REACT_VERSION" \
    ${EXTRA_DEPS:-}
  if [ "${INSTALL_REMOTION_BROWSER:-1}" = "1" ]; then
    printf '%s\n' "[runtime] fetching chrome headless shell"
    # --no-bin-links 不生成 node_modules/.bin，npx 找不到可执行，直接调用 CLI 入口。
    node "$remotion_dir/node_modules/@remotion/cli/remotion-cli.js" browser ensure
  fi
)

# 浏览器路径写成相对 remotion 根目录的形式，避免依赖固定的挂载点。
browser_rel=""
# amd64 重打包里的可执行文件叫 chrome-headless-shell，arm64 的叫 headless_shell。
browser_abs="$(find "$remotion_dir/node_modules/.remotion" -type f \( -name 'chrome-headless-shell' -o -name 'headless_shell' \) -print -quit 2>/dev/null || true)"
if [ -n "$browser_abs" ]; then
  browser_rel="${browser_abs#"$remotion_dir"/}"
fi
printf '%s' "$browser_rel" > "$remotion_dir/browser-path"

mkdir -p "$remotion_dir/bin"
printf '%s\n' \
  '#!/bin/sh' \
  '# 复用运行时自带的 Chrome Headless Shell，避免每个项目首次渲染都联网下载。' \
  '# 只有项目自己已经装了浏览器（node_modules/.remotion）时才不干预。' \
  'ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)' \
  'CLI="$ROOT/node_modules/@remotion/cli/remotion-cli.js"' \
  'SHELL_BIN="${WUNDER_REMOTION_BROWSER_BIN:-}"' \
  'if [ -z "$SHELL_BIN" ] && [ -s "$ROOT/browser-path" ]; then SHELL_BIN="$ROOT/$(cat "$ROOT/browser-path")"; fi' \
  'case "$1" in' \
  '  render|still|benchmark|compositions|gpu)' \
  '    if [ -n "$SHELL_BIN" ] && [ -x "$SHELL_BIN" ] && [ ! -e node_modules/.remotion ]; then' \
  '      case " $* " in' \
  '        *" --browser-executable"*) ;;' \
  '        *) exec node "$CLI" "$@" --browser-executable="$SHELL_BIN" ;;' \
  '      esac' \
  '    fi' \
  '    ;;' \
  'esac' \
  'exec node "$CLI" "$@"' \
  > "$remotion_dir/bin/remotion"
chmod +x "$remotion_dir/bin/remotion"

printf '%s\n' "[runtime] packing remotion"
tar -czf "$OUT_DIR/remotion.tar.gz" -C "$WORK" --transform 's,^remotion,remotion,' remotion

# ---------- 离线 npm 缓存 ----------
if [ -d "$WORK/npm-cache/_cacache" ]; then
  printf '%s\n' "[runtime] packing npm offline cache"
  tar -czf "$OUT_DIR/npm-cache.tar.gz" -C "$WORK/npm-cache" _cacache
fi

# ---------- 清单 ----------
# manifest.env 可能还有 Blender 等其他运行时的条目：保留非本脚本条目，更新本脚本条目。
{
  if [ -f "$OUT_DIR/manifest.env" ]; then
    grep -vE '^(NODE_VERSION|REMOTION_VERSION|REACT_VERSION|EXTRA_DEPS|BROWSER)=' "$OUT_DIR/manifest.env" || true
  fi
  printf '%s\n' \
    "NODE_VERSION=$NODE_VERSION" \
    "REMOTION_VERSION=$REMOTION_VERSION" \
    "REACT_VERSION=$REACT_VERSION" \
    "EXTRA_DEPS=${EXTRA_DEPS:-}" \
    "BROWSER=$browser_rel"
} > "$OUT_DIR/manifest.env.tmp"
mv "$OUT_DIR/manifest.env.tmp" "$OUT_DIR/manifest.env"

printf '%s\n' "[runtime] done"
ls -lh "$OUT_DIR"/*.tar.gz | awk '{print "  " $9, $5}'
