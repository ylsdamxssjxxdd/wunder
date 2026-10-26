#!/usr/bin/env bash
# One-time preparation of the kylin-x86 offline SDK sysroots, run inside an
# x86_64 Ubuntu 18.04 container:
#
#   docker run --rm --network host --platform linux/amd64 \
#     -v /path/to/wunder:/workspace:ro -v /path/to/kylin-x86:/output \
#     -w /workspace ubuntu:18.04 \
#     env WUNDER_OFFLINE_ROOT=/output/offline \
#     bash builders/prepare-kylin-x86-sysroots.sh
#
# Produces linux-amd64-ubuntu18/ (native cross-link sysroot for x86_64 hosts)
# and linux-arm64-ubuntu18/ (aarch64 cross target) with the same layout the
# kylin-arm SDK uses. The runtime package lists mirror
# builders/prepare-linux-amd64-runtime-sysroot.sh and
# builders/prepare-linux-arm64-runtime-sysroot.sh; keep them in sync.
set -euo pipefail

offline_root="${WUNDER_OFFLINE_ROOT:?WUNDER_OFFLINE_ROOT is required}"
offline_root="$(cd "$offline_root" && pwd -P)"

fail() { echo "[wunder-kylin-x86-sysroots] $*" >&2; exit 2; }
[[ "$(uname -m)" == x86_64 ]] || fail "run this on an x86_64 host"
[[ -r /etc/os-release ]] || fail "cannot identify Linux distribution"
. /etc/os-release
[[ "$ID:$VERSION_ID" == ubuntu:18.04 ]] || fail "Ubuntu 18.04 is required to preserve the GLIBC_2.27 release floor"

AMD64_RUNTIME_PACKAGES=(
  libasound2 libasound2-dev
  libxtst6
  libx11-6 libx11-xcb1 libxext6 libxfixes3 libxrender1 libxi6 libxrandr2 libxcursor1 libxinerama1
  libxau6 libxdmcp6
  libxcb1 libxcb-render0 libxcb-shape0 libxcb-xfixes0 libxcb-randr0 libxcb-xkb1
  libxcb-image0 libxcb-keysyms1 libxcb-icccm4 libxcb-util1 libxcb-shm0 libxcb-sync1
  libxcb-present0 libxcb-glx0 libxcb-dri2-0 libxcb-dri3-0
  libxkbcommon0 libxkbcommon-x11-0
)
ARM64_RUNTIME_PACKAGES=(
  libasound2 libasound2-dev
  libbsd0
  libxtst6
  libx11-6 libx11-xcb1 libxext6 libxfixes3 libxrender1 libxi6 libxrandr2 libxcursor1 libxinerama1
  libxau6 libxdmcp6
  libxcb1 libxcb-render0 libxcb-shape0 libxcb-xfixes0 libxcb-randr0 libxcb-xkb1
  libxcb-image0 libxcb-keysyms1 libxcb-icccm4 libxcb-util1 libxcb-shm0 libxcb-sync1
  libxcb-present0 libxcb-glx0 libxcb-dri2-0 libxcb-dri3-0
  libxkbcommon0 libxkbcommon-x11-0
  squashfs-tools liblz4-1 liblzo2-2 liblzma5 libzstd1 zlib1g
)
DEVEL_PACKAGES_AMD64=(
  libc6 libc6-dev linux-libc-dev libgcc1 libgcc-7-dev libstdc++6 libstdc++-7-dev
  cpp-7 gcc-7 binutils binutils-x86-64-linux-gnu libcc1-0
  libgmp10 libisl19 libmpc3 libmpfr6 zlib1g
)
DEVEL_PACKAGES_ARM64=(
  libc6 libc6-dev linux-libc-dev libgcc1 libgcc-7-dev libstdc++6 libstdc++-7-dev
  cpp-7 gcc-7 binutils binutils-aarch64-linux-gnu libcc1-0
  libgmp10 libisl19 libmpc3 libmpfr6 zlib1g
)

write_apt_sources() {
  cat >/etc/apt/sources.list <<'EOF'
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu bionic main universe multiverse restricted
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu bionic-updates main universe multiverse restricted
deb [trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu bionic-security main universe multiverse restricted
EOF
}

write_apt_ports_sources() {
  cat >/etc/apt/sources.list <<'EOF'
deb [arch=amd64 trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu bionic main universe multiverse restricted
deb [arch=amd64 trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu bionic-updates main universe multiverse restricted
deb [arch=amd64 trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu bionic-security main universe multiverse restricted
deb [arch=arm64 trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu-ports bionic main universe multiverse restricted
deb [arch=arm64 trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu-ports bionic-updates main universe multiverse restricted
deb [arch=arm64 trusted=yes] http://mirrors.tuna.tsinghua.edu.cn/ubuntu-ports bionic-security main universe multiverse restricted
EOF
}

apt_update() {
  printf 'Acquire::Check-Valid-Until "false";\n' >/etc/apt/apt.conf.d/99wunder-no-valid-until
  apt-get update -o Acquire::Check-Valid-Until=false
}

download_packages() {
  local cache_dir="$1" arch="$2"
  shift 2
  mkdir -p "$cache_dir/partial"
  apt-get install --download-only --reinstall --yes --no-install-recommends \
    -o Dir::Cache::archives="$cache_dir" \
    -o Dir::Cache::archives/partial="$cache_dir/partial" \
    "${@/%/:$arch}"
}

extract_and_repair() {
  local root="$1"
  shift
  local archive
  for archive in "$@"; do
    dpkg-deb -x "$archive" "$root"
  done
  while IFS= read -r -d '' link; do
    local target relative_target
    target="$(readlink "$link")"
    [[ "$target" == /* && -e "$root$target" ]] || continue
    relative_target="$(realpath --relative-to="$(dirname "$link")" "$root$target")"
    ln -sfn "$relative_target" "$link"
  done < <(find "$root" -type l -print0)
}

hash_debs() {
  local dir="$1"
  (cd "$dir" && sha256sum ./*.deb > SHA256SUMS)
}

prepare_amd64_sysroot() {
  local sdk="$offline_root/linux-amd64-ubuntu18"
  local root="$sdk/root"
  if [[ -f "$root/usr/x86_64-linux-gnu/lib/libgcc_eh.a" ]]; then
    printf '[wunder-kylin-x86-sysroots] amd64 sysroot already prepared, skipping\n'
    return 0
  fi
  local runtime_debs="$sdk/debs/runtime-amd64"
  local devel_debs="$sdk/debs/devel-amd64"
  mkdir -p "$root" "$runtime_debs" "$devel_debs"

  write_apt_sources
  apt_update
  apt-get install -y --no-install-recommends binutils
  download_packages "$runtime_debs" amd64 "${AMD64_RUNTIME_PACKAGES[@]}"
  (cd "$devel_debs" && apt-get download "${DEVEL_PACKAGES_AMD64[@]/%/:amd64}")

  shopt -s nullglob
  local archives=("$runtime_debs"/*.deb "$devel_debs"/*.deb)
  ((${#archives[@]})) || fail "apt did not download any amd64 packages"
  extract_and_repair "$root" "${archives[@]}"
  shopt -u nullglob
  hash_debs "$runtime_debs"
  hash_debs "$devel_debs"

  # Host gcc drivers locate crt objects and libgcc through -B; stage the
  # gcc-7/bionic payload under the multiarch lib dir the cross wrapper passes.
  local shim="$root/usr/x86_64-linux-gnu/lib"
  mkdir -p "$shim"
  cp -L "$root"/usr/lib/x86_64-linux-gnu/crt1.o \
        "$root"/usr/lib/x86_64-linux-gnu/Scrt1.o \
        "$root"/usr/lib/x86_64-linux-gnu/crti.o \
        "$root"/usr/lib/x86_64-linux-gnu/crtn.o \
        "$root"/usr/lib/x86_64-linux-gnu/Mcrt1.o \
        "$shim/" 2>/dev/null || true
  local gcc_lib="$root/usr/lib/gcc/x86_64-linux-gnu/7"
  local object
  for object in crtbegin.o crtbeginS.o crtbeginT.o crtend.o crtendS.o libgcc.a libgcc_eh.a libgcc_s.so; do
    [[ -f "$gcc_lib/$object" ]] && cp -L "$gcc_lib/$object" "$shim/"
  done
  cp -L "$root"/lib/x86_64-linux-gnu/libgcc_s.so.1 "$shim/"
  ln -sfn ../include "$root/usr/x86_64-linux-gnu/include"

  local readelf_tool
  readelf_tool="$(command -v readelf || true)"
  local library found
  for library in libasound.so.2 libasound.so libXtst.so.6 libX11.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0; do
    found=""
    for candidate in "$root"/usr/lib/x86_64-linux-gnu/"$library" "$root"/lib/x86_64-linux-gnu/"$library"; do
      [[ -e "$candidate" ]] && { found="$candidate"; break; }
    done
    [[ -n "$found" ]] || fail "runtime library was not installed into amd64 sysroot: $library"
    local machine
    machine="$(LC_ALL=C "$readelf_tool" -h "$found" | LC_ALL=C awk -F: '/Machine:/{print $2}' | LC_ALL=C sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
    [[ "$machine" == "Advanced Micro Devices X86-64" ]] || fail "$library is not an amd64 ELF: ${machine:-unknown}"
  done
  [[ -x "$gcc_lib/cc1" ]] || fail "amd64 sysroot GCC backend is incomplete: $gcc_lib/cc1"
  for object in crtbeginS.o libgcc.a libgcc_eh.a; do
    [[ -f "$shim/$object" ]] || fail "amd64 sysroot linker shim is missing: $shim/$object"
  done
  printf '[wunder-kylin-x86-sysroots] amd64 sysroot ready: %s (%s debs)\n' "$root" "${#archives[@]}"
}

prepare_arm64_sysroot() {
  local sdk="$offline_root/linux-arm64-ubuntu18"
  local root="$sdk/root"
  if [[ -f "$root/usr/lib/gcc/aarch64-linux-gnu/7/cc1" && -e "$root/usr/lib/aarch64-linux-gnu/libasound.so.2" ]]; then
    printf '[wunder-kylin-x86-sysroots] arm64 sysroot already prepared, skipping\n'
    return 0
  fi
  local runtime_debs="$sdk/debs/runtime-arm64"
  local devel_debs="$sdk/debs/devel-arm64"
  mkdir -p "$root" "$runtime_debs" "$devel_debs"

  dpkg --add-architecture arm64
  write_apt_ports_sources
  apt_update
  apt-get install -y --no-install-recommends binutils
  download_packages "$runtime_debs" arm64 "${ARM64_RUNTIME_PACKAGES[@]}"
  (cd "$devel_debs" && apt-get download "${DEVEL_PACKAGES_ARM64[@]/%/:arm64}")

  shopt -s nullglob
  local archives=("$runtime_debs"/*.deb "$devel_debs"/*.deb)
  ((${#archives[@]})) || fail "apt did not download any arm64 packages"
  extract_and_repair "$root" "${archives[@]}"
  shopt -u nullglob
  hash_debs "$runtime_debs"
  hash_debs "$devel_debs"

  local readelf_tool
  readelf_tool="$(command -v readelf || true)"
  local library found
  for library in libasound.so.2 libXtst.so.6 libX11.so.6 libxcb.so.1 libxcb-xkb.so.1 libxkbcommon.so.0 libxkbcommon-x11.so.0 libbsd.so.0; do
    found=""
    for candidate in "$root"/usr/lib/aarch64-linux-gnu/"$library" "$root"/lib/aarch64-linux-gnu/"$library"; do
      [[ -e "$candidate" ]] && { found="$candidate"; break; }
    done
    [[ -n "$found" ]] || fail "runtime library was not installed into arm64 sysroot: $library"
    local machine
    machine="$(LC_ALL=C "$readelf_tool" -h "$found" | LC_ALL=C awk -F: '/Machine:/{print $2}' | LC_ALL=C sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
    [[ "$machine" == "AArch64" ]] || fail "$library is not an arm64 ELF: ${machine:-unknown}"
  done
  [[ -x "$root/usr/lib/gcc/aarch64-linux-gnu/7/cc1" ]] || fail "arm64 sysroot GCC backend is incomplete"
  printf '[wunder-kylin-x86-sysroots] arm64 sysroot ready: %s (%s debs)\n' "$root" "${#archives[@]}"
}

prepare_amd64_sysroot
prepare_arm64_sysroot
