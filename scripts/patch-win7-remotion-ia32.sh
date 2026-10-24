#!/usr/bin/env bash
# 把 32 位 Remotion compositor 装进 win7 ia32 补充包，并修掉三处平台缺口：
#   1. @remotion/compositor-win32-ia32-msvc 包 + renderer 的 ia32 分支
#   2. 缺失的 @esbuild/win32-ia32（npm 以 --cpu=ia32 安装时不会自动补）
#   3. x264 线程上限（32 位 ffmpeg 在 1440p 以上会耗尽地址空间）
#
# 用法：
#   scripts/patch-win7-remotion-ia32.sh --root <package-root> --bin <compositor 产物目录> \
#     [--esbuild-tgz <@esbuild/win32-ia32 的 tgz>]
# root 是补充包解包后的根（其下有 opt/remotion、opt/node、opt/browser）。
set -euo pipefail

ROOT=""
REMOTION_ROOT=""
BIN=""
ESBUILD_TGZ=""
VERSION="4.0.538"

usage() {
  sed -n '2,12p' "$0"
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --root) ROOT="$2"; shift 2 ;;
    --remotion-root) REMOTION_ROOT="$2"; shift 2 ;;
    --bin) BIN="$2"; shift 2 ;;
    --esbuild-tgz) ESBUILD_TGZ="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) printf '%s\n' "unknown arg: $1" >&2; exit 2 ;;
  esac
done

[ -n "$BIN" ] && [ -f "$BIN/remotion.exe" ] || { printf '%s\n' "--bin 必须包含 remotion.exe" >&2; exit 2; }

if [ -n "$REMOTION_ROOT" ]; then
  ROOT="${ROOT:-$(dirname "$(dirname "$REMOTION_ROOT")")}"
  NODE_MODULES="$REMOTION_ROOT/node_modules"
elif [ -n "$ROOT" ] && [ -d "$ROOT" ]; then
  NODE_MODULES="$ROOT/opt/remotion/node_modules"
else
  printf '%s\n' "需要 --root <package-root> 或 --remotion-root <opt/remotion 目录>" >&2
  exit 2
fi
[ -d "$NODE_MODULES/@remotion/renderer" ] || { printf '%s\n' "不是有效的补充包：缺少 @remotion/renderer" >&2; exit 2; }

PKG="$NODE_MODULES/@remotion/compositor-win32-ia32-msvc"
rm -rf "$PKG"
mkdir -p "$PKG"
cp "$BIN"/*.exe "$BIN"/*.dll "$PKG"/
printf '%s\n' \
  '{' \
  '  "name": "@remotion/compositor-win32-ia32-msvc",' \
  "  \"version\": \"$VERSION\"," \
  '  "os": ["win32"],' \
  '  "cpu": ["ia32"],' \
  '  "exports": {' \
  '    ".": {' \
  '      "types": "./index.d.ts",' \
  '      "require": "./index.js"' \
  '    },' \
  '    "./package.json": "./package.json"' \
  '  }' \
  '}' > "$PKG/package.json"
printf '%s\n' 'exports.dir = __dirname;' > "$PKG/index.js"
printf '%s\n' 'export const dir: string;' > "$PKG/index.d.ts"
printf '%s\n' '# @remotion/compositor-win32-ia32-msvc' '' \
  'Internal package. Locally built 32-bit (i686) compositor for Windows 7 / ia32.' > "$PKG/README.md"

# renderer 只认 x64，补一个 ia32 分支（幂等）
EXEC_JS="$NODE_MODULES/@remotion/renderer/dist/compositor/get-executable-path.js"
if ! grep -q "compositor-win32-ia32-msvc" "$EXEC_JS"; then
  python - "$EXEC_JS" <<'PY'
import sys
path = sys.argv[1]
src = open(path, encoding='utf-8').read()
needle = "                case 'x64':\n                    return require('@remotion/compositor-win32-x64-msvc').dir;\n"
add = needle + "                case 'ia32':\n                    return require('@remotion/compositor-win32-ia32-msvc').dir;\n"
if needle not in src:
    raise SystemExit('renderer 结构与预期不符，未打补丁')
open(path, 'w', encoding='utf-8', newline='\n').write(src.replace(needle, add, 1))
PY
fi

# esbuild 的平台包：npm 以 --cpu=ia32 安装时会缺
if [ ! -f "$NODE_MODULES/@esbuild/win32-ia32/esbuild.exe" ] && [ -n "$ESBUILD_TGZ" ]; then
  mkdir -p "$NODE_MODULES/@esbuild/win32-ia32"
  tar -xzf "$ESBUILD_TGZ" -C "$NODE_MODULES/@esbuild/win32-ia32" --strip-components=1
fi

printf '%s\n' "patched $ROOT"
ls -1 "$PKG" | head -20

# x264 线程上限：包内 ffmpeg 是 32 位时，x264 默认线程数会把 4GB 地址空间吃穿
# （4K 实测 3716MB → 编码器 OOM，上层报 EPIPE）。幂等。
PRESPWN="$NODE_MODULES/@remotion/renderer/dist/prespawn-ffmpeg.js"
if [ -f "$PRESPWN" ] && [ -f "$PKG/ffmpeg.exe" ] && ! grep -q "wunder x264 thread cap" "$PRESPWN"; then
  python - "$PRESPWN" <<'PY'
import sys
path = sys.argv[1]
src = open(path, encoding='utf-8').read()
anchor = "        ...encodingArgsWithOwnedPixels,\n"
if anchor not in src:
    raise SystemExit('prespawn-ffmpeg.js 结构与预期不符，未打线程上限')
inject = anchor + """        // wunder x264 thread cap: the bundled encoder is 32-bit, so its address
        // space cannot hold x264's default thread pool at 1440p and above.
        ...(function () {
            var pixels = options.width * options.height;
            if (pixels >= 3840 * 2160) { return [['-threads:v', '6']]; }
            if (pixels >= 2560 * 1440) { return [['-threads:v', '8']]; }
            return [];
        })(),
"""
open(path, 'w', encoding='utf-8', newline='\n').write(src.replace(anchor, inject, 1))
print('x264 thread cap applied')
PY
fi
