#!/usr/bin/env bash
# Build the Linux x86_64 Remotion compositor in the same Ubuntu 18.04 image
# that builds Wunder's desktop binaries. The output is ready to replace
# @remotion/compositor-linux-x64-gnu in the Linux supplement.
set -euo pipefail

here="$(cd -- "$(dirname -- "$0")" && pwd)"
repo_root="$(cd -- "$here/../.." && pwd)"
work="${WUNDER_LINUX_COMPOSITOR_WORK:-$repo_root/temp/linux-ubuntu18-compositor}"
image="${WUNDER_LINUX_COMPOSITOR_IMAGE:-wunder-rust-builder-kylin-x86:ubuntu18}"
docker_bin="${WUNDER_DOCKER_BIN:-docker}"
remotion_version="${REMOTION_VERSION:-4.0.538}"
source_compositor="${SRC_COMPOSITOR:-}"
output="${WUNDER_LINUX_COMPOSITOR_OUTPUT:-$repo_root/config/runtime/linux-x64/compositor-linux-x64-gnu}"

log() { printf '[linux-compositor] %s\n' "$*"; }
fail() { printf '[linux-compositor] %s\n' "$*" >&2; exit 2; }
fetch() {
  [[ -s "$2" ]] || curl -fsSL --http1.1 --retry 5 "$1" -o "$2"
}

if [[ -z "$source_compositor" ]]; then
  for candidate in \
    "$repo_root/../参考/remotion-main/packages/compositor" \
    "$repo_root/../参考/remotion-main/remotion-main/packages/compositor"; do
    if [[ -f "$candidate/Cargo.toml" ]]; then
      source_compositor="$candidate"
      break
    fi
  done
fi

mkdir -p "$work/dl" "$work/src"
if [[ -z "$source_compositor" || ! -f "$source_compositor/Cargo.toml" ]]; then
  log "downloading Remotion v$remotion_version compositor source"
  archive="$work/dl/remotion-v$remotion_version.tar.gz"
  fetch "https://codeload.github.com/remotion-dev/remotion/tar.gz/refs/tags/v$remotion_version" "$archive"
  source_compositor="$work/source-compositor"
  rm -rf "$source_compositor"
  mkdir -p "$source_compositor"
  tar -xzf "$archive" -C "$source_compositor" --strip-components=3 \
    --wildcards '*/packages/compositor/*'
fi
[[ -f "$source_compositor/Cargo.toml" ]] || fail "Remotion compositor source unavailable"

destination_compositor="$work/src/compositor"
source_real="$(cd -- "$source_compositor" && pwd -P)"
destination_real="$(mkdir -p "$destination_compositor" && cd -- "$destination_compositor" && pwd -P)"
if [[ "$source_real" != "$destination_real" ]]; then
  rm -rf "$destination_compositor"
  mkdir -p "$destination_compositor"
  cp -a "$source_compositor/." "$destination_compositor/"
fi
source_compositor="$destination_compositor"
rm -rf "$work/src/compositor/target" "$work/src/compositor/toolchains"

rm -rf "$work/bindgen-gen" "$work/vendor"
cp -a "$here/bindgen-gen" "$work/bindgen-gen"
cp -a "$repo_root/scripts/win-ia32-compositor/vendor" "$work/vendor"
cp "$repo_root/scripts/win-ia32-compositor/fix-bindings.py" "$work/fix-bindings.py"
cp "$here/inner.sh" "$work/inner.sh"

# Cargo cannot access github.com in some builder environments. Source archives
# are fetched through codeload and patched locally by inner.sh.
fetch 'https://codeload.github.com/jonnyburger/mp4-rust/tar.gz/92ba375738cc2f05a4d754e1f968cf2e97d06641' \
  "$work/dl/mp4-rust.tar.gz"

log "building in $image"
"$docker_bin" run --rm \
  -v "$work:/b" \
  "$image" \
  bash -lc 'set -euo pipefail; export DEBIAN_FRONTEND=noninteractive; apt-get update -qq; apt-get install -y -qq --no-install-recommends libclang-dev clang; bash /b/inner.sh'

for name in remotion ffmpeg ffprobe libavcodec.so.61 libavformat.so.61 libavutil.so.59; do
  [[ -e "$work/out/$name" ]] || fail "compositor output missing: $name"
done

glibc_max="$("$docker_bin" run --rm -v "$work/out:/out:ro" "$image" bash -lc \
  'readelf --version-info /out/remotion | grep -o "GLIBC_[0-9.]*" | sort -Vu | tail -1')"
[[ "$glibc_max" < "GLIBC_2.28" ]] || fail "compositor requires $glibc_max, above Ubuntu 18.04"
log "remotion binary maximum ABI: $glibc_max"

rm -rf "$output"
mkdir -p "$output"
cp -a "$work/out/." "$output/"
log "output: $output"
