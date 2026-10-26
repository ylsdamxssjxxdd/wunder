#!/usr/bin/env bash
set -euo pipefail

# Build the Node/Remotion part of the Linux supplement in the newer runtime
# image. The resulting directory is merged into the Ubuntu 18.04 base pack by
# build_linux_supplement.sh.

repo_root="${WUNDER_REPO_ROOT:-/app}"
manifest_path="$repo_root/packaging/linux/scripts/linux-supplement-manifest.json"
output_root="${WUNDER_REMOTION_OUTPUT:-$repo_root/temp_dir/linux-remotion/opt}"
build_root="${WUNDER_REMOTION_BUILD_ROOT:-$repo_root/temp_dir/linux-remotion}"
mkdir -p "$output_root" "$build_root/downloads"

log() { echo "[linux-remotion] $*"; }
fail() { echo "[linux-remotion] $*" >&2; exit 2; }
require() { command -v "$1" >/dev/null 2>&1 || fail "required command unavailable: $1"; }
for tool in python3 curl tar sha256sum find file; do require "$tool"; done
[[ -f "$manifest_path" ]] || fail "manifest missing: $manifest_path"

case "${ARCH:-$(uname -m)}" in
  x86_64|amd64) arch=amd64; elf_machine=x86-64 ;;
  aarch64|arm64) arch=arm64; elf_machine=aarch64 ;;
  *) fail "unsupported architecture: ${ARCH:-$(uname -m)}" ;;
esac

json_field() {
  python3 - "$manifest_path" "$1" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
for token in sys.argv[2].split("."):
    value = value[int(token)] if isinstance(value, list) else value[token]
print(value if not isinstance(value, (dict, list)) else json.dumps(value, ensure_ascii=False))
PY
}

node_version="$(json_field node.version)"
node_url="$(json_field "node.archives.$arch.url")"
node_file="$(json_field "node.archives.$arch.fileName")"
node_sha="$(json_field "node.archives.$arch.sha256")"
remotion_version="$(json_field remotion.version)"
compositor="$(json_field "remotion.compositorPackages.$arch")"
packages="$(json_field remotion.packages)"
esbuild_version="$(cd "$repo_root" && python3 - <<'PY'
import json
pkg = json.load(open('packaging/linux/scripts/linux-supplement-manifest.json', encoding='utf-8'))
print(pkg.get('remotion', {}).get('esbuildVersion', '0.28.1'))
PY
)"
downloads="$build_root/downloads"
node_archive="$downloads/$node_file"

if [[ ! -f "$node_archive" ]] || ! echo "$node_sha  $node_archive" | sha256sum -c --status >/dev/null 2>&1; then
  log "downloading Node.js $node_version"
  curl -fL --retry 5 --retry-delay 3 --connect-timeout 30 -o "$node_archive.tmp" "$node_url"
  echo "$node_sha  $node_archive.tmp" | sha256sum -c --status
  mv -f "$node_archive.tmp" "$node_archive"
fi

rm -rf "$output_root/node" "$output_root/remotion" "$build_root/node-extract"
mkdir -p "$output_root/node" "$output_root/remotion" "$build_root/node-extract"
tar -xzf "$node_archive" -C "$build_root/node-extract"
node_src="$(find "$build_root/node-extract" -mindepth 1 -maxdepth 1 -type d -name 'node-v*' -print -quit)"
[[ -n "$node_src" ]] || fail "Node archive layout unexpected"
cp -a "$node_src/." "$output_root/node/"
node_bin="$output_root/node/bin/node"
npm_cli="$output_root/node/lib/node_modules/npm/bin/npm-cli.js"
[[ -x "$node_bin" && -f "$npm_cli" ]] || fail "Node/npm missing"
echo "$(file -bL "$node_bin")" | grep -qi "$elf_machine" || fail "Node architecture mismatch"
[[ "$($node_bin --version)" == "v$node_version" ]] || fail "Node smoke failed"

# The official tarball may ship bin/npm and bin/npx as symlinks into npm's
# library. Remove those links before creating relocatable wrappers; otherwise
# redirecting the wrapper would overwrite npm-cli.js itself.
rm -f "$output_root/node/bin/npm" "$output_root/node/bin/npx"
cat > "$output_root/node/bin/npm" <<'EOF_NPM'
#!/bin/sh
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$here/node" "$here/../lib/node_modules/npm/bin/npm-cli.js" "$@"
EOF_NPM
cat > "$output_root/node/bin/npx" <<'EOF_NPX'
#!/bin/sh
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$here/node" "$here/../lib/node_modules/npm/bin/npx-cli.js" "$@"
EOF_NPX
chmod 0755 "$output_root/node/bin/npm" "$output_root/node/bin/npx"

cat > "$output_root/remotion/package.json" <<EOF_PACKAGE
{
  "name": "wunder-remotion-runtime",
  "private": true,
  "version": "$remotion_version",
  "dependencies": $packages,
  "optionalDependencies": { "$compositor": "$remotion_version" }
}
EOF_PACKAGE
log "installing Remotion $remotion_version ($compositor)"
(cd "$output_root/remotion" && NPM_CONFIG_REGISTRY="${WUNDER_SUPPLEMENT_NPM_REGISTRY:-https://registry.npmjs.org}" \
  "$node_bin" "$npm_cli" install --omit=dev --no-audit --no-fund --ignore-scripts)

# npm 8 on Node 16 can incorrectly omit platform optional dependencies when a
# package declares a large optionalDependencies matrix. Install the selected
# esbuild binary explicitly so bundling works offline in the final pack.
case "$arch" in
  amd64) esbuild_platform="@esbuild/linux-x64" ;;
  arm64) esbuild_platform="@esbuild/linux-arm64" ;;
esac
(cd "$output_root/remotion" && NPM_CONFIG_REGISTRY="${WUNDER_SUPPLEMENT_NPM_REGISTRY:-https://registry.npmjs.org}" \
  "$node_bin" "$npm_cli" install --omit=dev --no-save --no-audit --no-fund --ignore-scripts \
    "$esbuild_platform@$esbuild_version")

cli="$output_root/remotion/node_modules/@remotion/cli/remotion-cli.js"
compositor_root="$output_root/remotion/node_modules/$compositor"
[[ -f "$cli" && -f "$compositor_root/ffmpeg" ]] || fail "Remotion packages incomplete"
if [[ "$(json_field remotion.browser.install)" == "True" || "$(json_field remotion.browser.install)" == "true" ]]; then
  (cd "$output_root/remotion" && "$node_bin" "$cli" browser ensure)
fi
# amd64 重打包里的可执行文件叫 chrome-headless-shell，arm64 的叫 headless_shell。
browser="$(find "$output_root/remotion/node_modules/.remotion" -type f \
  \( -name chrome-headless-shell -o -name headless_shell \) -print -quit 2>/dev/null || true)"
[[ -n "$browser" ]] || fail "Headless Shell was not installed"
printf '%s\n' "${browser#"$output_root/remotion"/}" > "$output_root/remotion/browser-path"
mkdir -p "$output_root/remotion/bin"
cat > "$output_root/remotion/bin/remotion" <<'EOF_REMOTION'
#!/bin/sh
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
NODE="$ROOT/../node/bin/node"
CLI="$ROOT/node_modules/@remotion/cli/remotion-cli.js"
COMPOSITOR="$ROOT/node_modules/@remotion/compositor-linux-x64-gnu"
if [ -d "$ROOT/node_modules/@remotion/compositor-linux-arm64-gnu" ]; then
  COMPOSITOR="$ROOT/node_modules/@remotion/compositor-linux-arm64-gnu"
fi
if [ -n "${LD_LIBRARY_PATH:-}" ]; then
  export LD_LIBRARY_PATH="$COMPOSITOR:${LD_LIBRARY_PATH}"
else
  export LD_LIBRARY_PATH="$COMPOSITOR"
fi
# The Linux supplement places Chrome beside Remotion. Prefer it when present;
# callers can override with WUNDER_BROWSER_BIN or an explicit Remotion flag.
BROWSER="${WUNDER_BROWSER_BIN:-}"
if [ -z "$BROWSER" ] && [ -x "$ROOT/../browser/chrome-linux64/chrome" ]; then
  BROWSER="$ROOT/../browser/chrome-linux64/chrome"
fi
if [ -z "$BROWSER" ] && [ -x "$ROOT/../browser/chrome-linux/chrome" ]; then
  BROWSER="$ROOT/../browser/chrome-linux/chrome"
fi
if [ -n "$BROWSER" ]; then
  NEEDS_BROWSER=0
  case "${1:-}" in
    render|studio|still|preview|compositions) NEEDS_BROWSER=1 ;;
  esac
  HAS_BROWSER_FLAG=0
  for arg in "$@"; do
    case "$arg" in
      --browser-executable|--browser-executable=*) HAS_BROWSER_FLAG=1; break ;;
    esac
  done
  if [ "$NEEDS_BROWSER" -eq 1 ] && [ "$HAS_BROWSER_FLAG" -eq 0 ]; then
    set -- "$@" "--browser-executable=$BROWSER"
  fi
fi
exec "$NODE" "$CLI" "$@"
EOF_REMOTION
chmod 0755 "$output_root/remotion/bin/remotion"
remotion_smoke="$(cd "$output_root/remotion" && "$node_bin" "$cli" versions 2>&1)"
[[ "$remotion_smoke" == *"$remotion_version"* ]] || fail "Remotion smoke failed: $remotion_smoke"
log "created $output_root/node and $output_root/remotion"
