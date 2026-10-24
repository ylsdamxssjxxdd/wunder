#!/bin/bash
# 容器内执行的部分：x264 → FFmpeg 7.1 → ffmpeg-sys 绑定 → compositor。
# 由 build.sh 通过 docker exec 调用，工作目录 /b（挂在宿主 temp/win-ia32-compositor）。
#
# 默认使用与 wunder 主程序相同的 Win7 工具链：
#   host   nightly-2026-03-14-x86_64（64 位 host 避免 LLVM 32 位地址空间限制）
#   target i686-win7-windows-gnu（Rust 1.78+ 内置的 Win7 target）
#   -Z build-std=std,panic_abort（tier 3 无预编译 std，从源码编）
# 这样 compositor 与 wunder 走同一套 ABI 假设，std 里不引用 Win10 API。
set -euo pipefail

RUST_TOOLCHAIN="${RUST_TOOLCHAIN:-nightly-2026-03-14}"
RUST_TARGET="${RUST_TARGET:-i686-win7-windows-gnu}"
# clang 不认识 win7 后缀，头文件预处理按普通 mingw 走
CLANG_TARGET="${CLANG_TARGET:-i686-pc-windows-gnu}"
# 32 位进程默认只有 2GB 用户地址空间；LARGEADDRESSAWARE 让它在 64 位 Windows 上用到 4GB
LINK_ARGS="${LINK_ARGS:--C link-arg=-Wl,--large-address-aware}"
JOBS="$(nproc)"

# 全部可复用产物落在挂载卷上：容器重建后 x264/FFmpeg/vendor/cargo 缓存还在，
# CI 按脚本哈希缓存 temp/win-ia32-compositor 就能省掉二十多分钟。
WORK=/b
VENDOR="$WORK/vendor"
SYS="$VENDOR/rust-ffmpeg-sys"
GEN="$WORK/gen"
export CARGO_TARGET_DIR="$WORK/target"
export SYS_ROOT="$SYS"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_PATH="$WORK/i686/x264/lib/pkgconfig:$WORK/i686/ffmpeg/lib/pkgconfig"

log() { printf '%s\n' "[ia32] $*"; }

# ---------- 1. 源码 ----------
mkdir -p dl src i686
fetch() { [ -s "$2" ] || curl -fsSL --http1.1 --retry 5 --retry-all-errors "$1" -o "$2"; }
fetch https://codeload.github.com/mirror/x264/tar.gz/refs/heads/stable dl/x264.tar.gz
fetch https://codeload.github.com/FFmpeg/FFmpeg/tar.gz/refs/tags/n7.1 dl/ffmpeg.tar.gz
[ -d src/x264 ] || { mkdir -p src/x264; tar -xf dl/x264.tar.gz -C src/x264 --strip-components=1; }
[ -d src/ffmpeg ] || { mkdir -p src/ffmpeg; tar -xf dl/ffmpeg.tar.gz -C src/ffmpeg --strip-components=1; }

# ---------- 2. x264（静态）----------
if [ ! -f i686/x264/lib/libx264.a ]; then
  log "x264"
  ( cd src/x264 &&
    ./configure --host=i686-w64-mingw32 --cross-prefix=i686-w64-mingw32- \
      --prefix="$WORK/i686/x264" --enable-static --disable-cli --disable-opencl >/dev/null &&
    make -j"$JOBS" >/dev/null && make install >/dev/null )
fi

# ---------- 3. FFmpeg 7.1（共享 DLL）----------
if [ ! -f i686/ffmpeg/bin/avcodec-61.dll ]; then
  log "ffmpeg 7.1"
  ( cd src/ffmpeg &&
    ./configure --prefix="$WORK/i686/ffmpeg" --arch=x86 --target-os=mingw32 \
      --enable-cross-compile --cross-prefix=i686-w64-mingw32- --pkg-config=pkg-config \
      --enable-shared --disable-static --enable-gpl --enable-libx264 \
      --disable-doc --disable-debug --disable-ffplay --disable-iconv \
      --extra-cflags="-I$WORK/i686/x264/include" --extra-ldflags="-L$WORK/i686/x264/lib" >/tmp/ffconf.log 2>&1 ||
      { tail -20 /tmp/ffconf.log >&2; exit 1; }
    make -j"$JOBS" >/tmp/ffbuild.log 2>&1 || { tail -30 /tmp/ffbuild.log >&2; exit 1; }
    make install >/dev/null )
fi

# ---------- 4. ffmpeg-sys 源码（仓库 3.5GB，只取源码）----------
if [ ! -d "$SYS/src" ]; then
  log "vendor ffmpeg-sys"
  mkdir -p "$VENDOR"
  python3 "$WORK/fetch-sys.py"
fi

# 官方只映射 x86_64 windows，补 ia32；win7 target 是独立 triple，也要一条
python3 - "$SYS/build.rs" "$RUST_TARGET" <<'PY'
import sys
path, target = sys.argv[1], sys.argv[2]
s = open(path, encoding="utf-8").read()
anchor = '            "x86_64-pc-windows-gnu" => "windows.gz".to_string(),'
if target not in s:
    assert anchor in s, "build.rs 结构变了，需要人工检查"
    add = anchor + f'\n            "{target}" => "{target}.gz".to_string(),'
    open(path, "w", encoding="utf-8", newline="\n").write(s.replace(anchor, add, 1))
PY

mkdir -p "$SYS/zips"
if [ ! -f "$SYS/zips/$RUST_TARGET.gz" ]; then
  tar -czf "$SYS/zips/$RUST_TARGET.gz" -C "$WORK/i686" \
    --transform 's,^ffmpeg,remotion,' ffmpeg
fi

# ---------- 5. 32 位 bindings ----------
log "bindgen ($CLANG_TARGET)"
mkdir -p "$GEN/src" "$SYS/bindings"
cp "$WORK/src/bindgen-gen/Cargo.toml" "$GEN/Cargo.toml"
cp "$WORK/src/bindgen-gen/src/main.rs" "$GEN/src/main.rs"
( cd "$GEN" && cargo build --release >/dev/null )

cat > "$VENDOR/wrapper.h" <<'EOF'
#include <libavcodec/avcodec.h>
#include <libavdevice/avdevice.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavformat/avformat.h>
#include <libavutil/avutil.h>
#include <libavutil/imgutils.h>
#include <libavutil/opt.h>
#include <libavutil/time.h>
#include <libavutil/channel_layout.h>
#include <libavutil/hwcontext.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>
EOF

BINDINGS="$SYS/bindings/$RUST_TARGET.rs"
BINDGEN_TARGET="$CLANG_TARGET" "$CARGO_TARGET_DIR/release/bindgen-gen" \
  "$VENDOR/wrapper.h" "$BINDINGS" "$WORK/i686/ffmpeg/include"

# bindgen 求不出「枚举移位」型宏，通道布局常量用 C 端求值补齐
{
  echo "#include <stdio.h>"
  echo "#include <libavutil/channel_layout.h>"
  echo "int main(void){"
  grep -oE "^#define (AV_CH_LAYOUT_[A-Z0-9_]+)" "$WORK/i686/ffmpeg/include/libavutil/channel_layout.h" |
    awk '{print $2}' |
    while read -r n; do echo "printf(\"$n %llu\\n\", (unsigned long long)$n);"; done
  echo "return 0;}"
} > /tmp/ch.c
gcc /tmp/ch.c -I"$WORK/i686/ffmpeg/include" -o /tmp/ch
/tmp/ch > /tmp/ch.txt
{ echo ""; awk '{printf "pub const %s: u64 = %s;\n", $1, $2}' /tmp/ch.txt; } >> "$BINDINGS"

python3 "$WORK/fix-bindings.py" "$BINDINGS"

# ---------- 6. 编译 ----------
log "cargo build ($RUST_TOOLCHAIN → $RUST_TARGET)"
cd "$WORK/crate"
# 幂等地把两个 git 依赖指向 vendored 副本。容器里没有 github.com 直连
# （只有 codeload/api 通），cargo 自己拉 git 依赖会失败。
python3 - "$WORK/crate/Cargo.toml" "$VENDOR" <<'PY'
import re
import sys

path, vendor = sys.argv[1], sys.argv[2]
src = open(path, encoding="utf-8").read()
for url in (
    "https://github.com/remotion-dev/rust-ffmpeg",
    "https://github.com/remotion-dev/rust-ffmpeg-sys",
):
    src = re.sub(
        r'\n\[patch\."' + re.escape(url) + r'"\]\n(?:[^\[\n][^\n]*\n)*',
        '\n',
        src,
    )
section = (
    f'\n[patch."https://github.com/remotion-dev/rust-ffmpeg"]\n'
    f'ffmpeg-next = {{ path = "{vendor}/rust-ffmpeg" }}\n'
    f'\n[patch."https://github.com/remotion-dev/rust-ffmpeg-sys"]\n'
    f'ffmpeg-sys-next = {{ path = "{vendor}/rust-ffmpeg-sys" }}\n'
)
src = src.rstrip("\n") + "\n" + section
open(path, "w", encoding="utf-8", newline="\n").write(src)
PY

# 32 位帧缓存上限：不改会在 4K 下把 4GB 地址空间吃穿
python3 "$WORK/patch-compositor.py" "$WORK/crate"

# Remotion 4.0.538 的 lock 里 proc-macro2 停在 1.0.56，它用的
# proc_macro_span_shrink feature 已被新版 nightly 移除，必须抬一次版本。
cargo +"$RUST_TOOLCHAIN" update -p proc-macro2 >/dev/null 2>&1 || true

export CARGO_TARGET_I686_WIN7_WINDOWS_GNU_LINKER=i686-w64-mingw32-gcc
export CARGO_TARGET_I686_PC_WINDOWS_GNU_LINKER=i686-w64-mingw32-gcc
export FFMPEG_DIR="$WORK/i686/ffmpeg"
RUSTFLAGS="$LINK_ARGS" cargo +"$RUST_TOOLCHAIN" -Z build-std=std,panic_abort \
  build --release --target "$RUST_TARGET"

# ---------- 7. 组装 ----------
# 注意：此时 cwd 是 $WORK/crate，必须用绝对路径引用 FFmpeg 产物。
OUT="$WORK/out/compositor-win32-ia32"
rm -rf "$OUT" && mkdir -p "$OUT"
cp "$CARGO_TARGET_DIR/$RUST_TARGET/release/remotion.exe" "$OUT/"
cp "$WORK"/i686/ffmpeg/bin/*.dll "$WORK/i686/ffmpeg/bin/ffmpeg.exe" "$WORK/i686/ffmpeg/bin/ffprobe.exe" "$OUT/"
for d in libgcc_s_dw2-1.dll libstdc++-6.dll libwinpthread-1.dll; do
  p="$(i686-w64-mingw32-gcc -print-file-name=$d)"
  [ -f "$p" ] && cp "$p" "$OUT/"
done
log "done"
ls -lh "$OUT"
