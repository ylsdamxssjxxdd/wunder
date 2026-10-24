#!/usr/bin/env bash
# Runs inside the project Ubuntu 18.04 Rust builder. It builds the Remotion
# compositor and its FFmpeg closure for the desktop glibc 2.27 baseline.
set -euo pipefail

work=/b
arch=x86_64
target=x86_64-unknown-linux-gnu
toolchain="${RUST_TOOLCHAIN:-nightly-2026-03-14}"
jobs="$(nproc)"
cd "$work"

log() { printf '[linux-compositor] %s\n' "$*"; }
fail() { printf '[linux-compositor] %s\n' "$*" >&2; exit 2; }
fetch() {
  [[ -s "$2" ]] || curl -fsSL --http1.1 --retry 5 "$1" -o "$2"
}

mkdir -p dl src "$arch" vendor/rust-ffmpeg-sys/zips
export PKG_CONFIG_PATH="$work/$arch/x264/lib/pkgconfig:$work/$arch/ffmpeg/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

if [[ ! -f "$arch/x264/lib/libx264.a" ]]; then
  log 'building x264'
  fetch 'https://codeload.github.com/mirror/x264/tar.gz/refs/heads/stable' dl/x264.tar.gz
  rm -rf src/x264
  mkdir -p src/x264
  tar -xf dl/x264.tar.gz -C src/x264 --strip-components=1
  (
    cd src/x264
    ./configure --host=x86_64-linux-gnu --prefix="$work/$arch/x264" \
      --enable-static --enable-pic --disable-cli --disable-opencl
    make -j"$jobs"
    make install
  )
fi

if [[ ! -f "$arch/ffmpeg/lib/libavcodec.so.61" ]]; then
  log 'building FFmpeg 7.1'
  fetch 'https://codeload.github.com/FFmpeg/FFmpeg/tar.gz/refs/tags/n7.1' dl/ffmpeg.tar.gz
  rm -rf src/ffmpeg
  mkdir -p src/ffmpeg
  tar -xf dl/ffmpeg.tar.gz -C src/ffmpeg --strip-components=1
  (
    cd src/ffmpeg
    ./configure --prefix="$work/$arch/ffmpeg" --arch=x86_64 --target-os=linux \
      --enable-shared --disable-static --enable-gpl --enable-libx264 \
      --disable-doc --disable-debug --disable-ffplay --disable-iconv \
      --disable-outdev=xcb --disable-indev=xcb \
      --extra-cflags="-I$work/$arch/x264/include" \
      --extra-ldflags="-L$work/$arch/x264/lib"
    make -j"$jobs"
    make install
  )
fi

# rust-ffmpeg-sys expects a directory named remotion in the compressed tree.
tar -czf vendor/rust-ffmpeg-sys/zips/linux-x64.gz -C "$arch" \
  --transform 's,^ffmpeg,remotion,' ffmpeg

# Its build script mutates TARGET from the Rust target to linux-x64 before
# resolving the archive a second time; supply both spellings. Open the archive
# relative to CARGO_MANIFEST_DIR because Cargo runs build scripts elsewhere.
cp -f vendor/rust-ffmpeg-sys/zips/linux-x64.gz \
  vendor/rust-ffmpeg-sys/zips/x86_64-unknown-linux-gnu.gz
python3 - <<'PY'
from pathlib import Path

path = Path('vendor/rust-ffmpeg-sys/build.rs')
src = path.read_text(encoding='utf-8')
old = 'let input_file = File::open(format!("zips/{}", get_zip_name())).unwrap();'
new = ('let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");\n'
       '    let input_file = File::open(PathBuf::from(&manifest_dir).join("zips").join(get_zip_name())).unwrap();')
if old in src:
    src = src.replace(old, new, 1)
    src = src.replace(
        '    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set");\n'
        '    let ffmpeg_dir = PathBuf::from(manifest_dir)',
        '    let ffmpeg_dir = PathBuf::from(manifest_dir)',
        1,
    )
path.write_text(src, encoding='utf-8', newline='\n')
PY

mkdir -p gen/src vendor/rust-ffmpeg-sys/bindings
cp bindgen-gen/Cargo.toml gen/Cargo.toml
cp bindgen-gen/src/main.rs gen/src/main.rs
export LIBCLANG_PATH="${LIBCLANG_PATH:-/usr/lib/llvm-6.0/lib}"
cargo build --manifest-path gen/Cargo.toml --release

cat > vendor/wrapper.h <<'EOF'
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

bindings="vendor/rust-ffmpeg-sys/bindings/$target.rs"
BINDGEN_TARGET="$target" gen/target/release/bindgen-gen \
  vendor/wrapper.h "$bindings" "$arch/ffmpeg/include"

# bindgen cannot evaluate the AV_CH_LAYOUT_* bitwise macros. Evaluate them
# with the just-built C headers, then append Rust constants expected by
# rust-ffmpeg.
{
  echo '#include <stdio.h>'
  echo '#include <libavutil/channel_layout.h>'
  echo 'int main(void) {'
  grep -oE '^#define (AV_CH_LAYOUT_[A-Z0-9_]+)' "$arch/ffmpeg/include/libavutil/channel_layout.h" |
    awk '{print $2}' |
    while read -r name; do
      echo "printf(\"$name %llu\\n\", (unsigned long long)$name);"
    done
  echo 'return 0;}'
} > /tmp/remotion-channel-layouts.c
gcc /tmp/remotion-channel-layouts.c -I"$arch/ffmpeg/include" -o /tmp/remotion-channel-layouts
/tmp/remotion-channel-layouts > /tmp/remotion-channel-layouts.txt
{ echo; awk '{printf "pub const %s: u64 = %s;\n", $1, $2}' /tmp/remotion-channel-layouts.txt; } >> "$bindings"
python3 fix-bindings.py "$bindings"

mkdir -p vendor/mp4-rust
tar -xf dl/mp4-rust.tar.gz -C vendor/mp4-rust --strip-components=1

cd src/compositor
python3 - "$PWD/Cargo.toml" "$work/vendor" <<'PY'
from pathlib import Path
import sys

manifest, vendor = map(Path, sys.argv[1:])
src = manifest.read_text(encoding='utf-8').split('\n[patch.', 1)[0].rstrip()
src += f'''\n[patch."https://github.com/remotion-dev/rust-ffmpeg"]
ffmpeg-next = {{ path = "{vendor}/rust-ffmpeg" }}
[patch."https://github.com/remotion-dev/rust-ffmpeg-sys"]
ffmpeg-sys-next = {{ path = "{vendor}/rust-ffmpeg-sys" }}
[patch."https://github.com/jonnyburger/mp4-rust"]
mp4 = {{ path = "{vendor}/mp4-rust" }}
'''
manifest.write_text(src, encoding='utf-8', newline='\n')
PY

export CARGO_TARGET_DIR="$work/target"
export FFMPEG_DIR="$work/$arch/ffmpeg"
# Remotion 4.0.538 locks proc-macro2 to a release that current nightly no
# longer supports. Updating that one dependency leaves the source API intact.
rustup run "$toolchain" cargo update -p proc-macro2 || true
RUSTFLAGS='-C link-arg=-Wl,-rpath,$ORIGIN' \
  rustup run "$toolchain" cargo build --release --target "$target"

out="$work/out"
rm -rf "$out"
mkdir -p "$out"
cp "$work/target/$target/release/remotion" "$out/remotion"
cp -a "$work/$arch/ffmpeg/lib/." "$out/"
cp "$work/$arch/ffmpeg/bin/ffmpeg" "$work/$arch/ffmpeg/bin/ffprobe" "$out/"

# FFmpeg's development symlinks must remain symlinks or the package grows by
# tens of megabytes. Every executable/shared object resolves sibling libs.
find "$out" -type f \( -name 'remotion' -o -name 'ffmpeg' -o -name 'ffprobe' -o -name '*.so.*' \) \
  -exec patchelf --set-rpath '$ORIGIN' {} \;
log "built $out"
