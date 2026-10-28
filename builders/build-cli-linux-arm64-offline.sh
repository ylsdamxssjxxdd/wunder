#!/usr/bin/env bash
# Build the Linux ARM64 wunder CLI. ARM64 hosts build natively; x86_64 hosts
# cross-build with the host tools shipped in the kylin-x86 SDK.
# CLI distribution is intentionally a plain executable, never an AppImage.
set -euo pipefail

repo_root="${WUNDER_REPO_ROOT:-$(cd -- "$(dirname -- "$0")/.." && pwd)}"
host_arch="$(uname -m)"
case "$host_arch" in
  aarch64) host_profile=kylin-arm ;;
  x86_64) host_profile=kylin-x86 ;;
  *) echo "[wunder-cli-linux-arm64] unsupported Linux host: ${host_arch:-unknown}" >&2; exit 2 ;;
esac
builder_root="${WUNDER_BUILDER_ROOT:-$repo_root/../Rust-builder/$host_profile}"
offline_root="${WUNDER_OFFLINE_ROOT:-$builder_root/offline}"
if [[ "$host_arch" == x86_64 ]]; then
  rust="$offline_root/rust/toolchains/nightly-2026-03-14-x86_64-unknown-linux-gnu"
  # Accept the initial SDK export with an extra directory level.
  [[ -x "$rust/bin/cargo" ]] || rust="$rust/nightly-2026-03-14-x86_64-unknown-linux-gnu"
else
  rust="$offline_root/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu"
fi
vendor_root="${WUNDER_CARGO_VENDOR:-$offline_root/cargo-vendor-slint}"
target="aarch64-unknown-linux-gnu"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/cli-linux-arm64/cargo}"
cargo_home="${CARGO_HOME:-$repo_root/target/cli-linux-arm64/cargo-home}"
output_dir="${WUNDER_CLI_OUTPUT_DIR:-$repo_root/target/cli/dist/linux-arm64}"
max_glibc="${WUNDER_CLI_LINUX_MAX_GLIBC:-2.27}"
sysroot="$offline_root/linux-arm64-ubuntu18/root"
sysroot_lib="$sysroot/usr/lib/aarch64-linux-gnu"
gcc_lib="$sysroot/usr/lib/gcc/aarch64-linux-gnu/7"

fail() { echo "[wunder-cli-linux-arm64] $*" >&2; exit 2; }
require_file() { [[ -f "$1" ]] || fail "required file is missing: $1"; }
require_command() { command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"; }

require_file "$repo_root/Cargo.toml"
[[ -x "$rust/bin/cargo" ]] || fail "Rust toolchain is missing: $rust/bin/cargo"
[[ -d "$vendor_root" ]] || fail "shared offline Cargo vendor is missing: $vendor_root"
[[ -f "$sysroot_lib/crt1.o" && -f "$sysroot_lib/libc.so" ]] || fail "Ubuntu 18 ARM64 development sysroot is incomplete: $sysroot"
gcc_shared="$gcc_lib/libgcc_s.so"
[[ ! -L "$gcc_shared" || "$(readlink "$gcc_shared")" != /* ]] \
  || fail "Ubuntu 18 ARM64 sysroot has an absolute libgcc_s.so link: $gcc_lib (run builders/prepare-linux-arm64-devel-sysroot.sh --repair)"
[[ -e "$gcc_shared" ]] \
  || fail "Ubuntu 18 ARM64 sysroot has a dangling libgcc_s.so link: $gcc_lib (run builders/prepare-linux-arm64-devel-sysroot.sh --repair)"
for library in libgmp.so.10 libisl.so.19 libmpc.so.3 libmpfr.so.6; do
  [[ -e "$sysroot/usr/lib/aarch64-linux-gnu/$library" || -e "$sysroot/lib/aarch64-linux-gnu/$library" ]] \
    || fail "ARM64 SDK GCC runtime is missing: $library (run builders/prepare-linux-arm64-devel-sysroot.sh)"
done

# The x86_64 cross compiler, linker and binutils come from host-tools-amd64;
# a native ARM64 host keeps using the SDK's own toolchain.
strip_cmd=(strip)
cargo_target_args=()
if [[ "$host_arch" == x86_64 ]]; then
  host_root="$(bash "$repo_root/builders/prepare-x86-arm64-cross.sh")"
  export WUNDER_ARM64_CROSS_HOST_ROOT="$host_root"
  export PATH="$rust/bin:$host_root/usr/bin:$PATH"
  strip_cmd=(env "LD_LIBRARY_PATH=$host_root/usr/lib/x86_64-linux-gnu" "$host_root/usr/bin/aarch64-linux-gnu-strip")
  cargo_target_args=(--target "$target")
else
  [[ -x "$gcc_lib/cc1" ]] \
    || fail "ARM64 SDK GCC backend is missing: $gcc_lib/cc1 (run builders/prepare-linux-arm64-devel-sysroot.sh)"
  export PATH="$rust/bin:$PATH"
fi
for tool in cargo rustc readelf awk grep sed sort tail; do require_command "$tool"; done
if [[ "$host_arch" == x86_64 ]]; then
  require_command aarch64-linux-gnu-gcc-7
  require_command aarch64-linux-gnu-strip
fi

# CLI --config outranks every config file: cargo >=1.96 resolves its config
# home from the passwd entry rather than HOME or CARGO_HOME, so a
# developer-wide ~/.cargo/config.toml (mirror setups) would otherwise defeat
# the vendored configuration written into the per-build cargo home.
WUNDER_VENDORED_CONFIG=(--config 'source.crates-io.replace-with="vendored-sources"')
# The repo may be checked out without the executable bit on builder scripts;
# cargo needs a directly executable linker wrapper.
arm64_cc="$repo_root/builders/linux_arm64_sysroot_cc.sh"
if [[ ! -x "$arm64_cc" ]]; then
  mkdir -p "$target_dir"
  arm64_cc="$target_dir/linux_arm64_sysroot_cc.sh"
  cp -f "$repo_root/builders/linux_arm64_sysroot_cc.sh" "$arm64_cc"
  chmod 755 "$arm64_cc"
fi
export CARGO_HOME="$cargo_home"
export CARGO_TARGET_DIR="$target_dir"
export CARGO_NET_OFFLINE=true
export RCHO_ARM64_SYSROOT="$sysroot"
export RCHO_ARM64_CC="$sysroot/usr/bin/aarch64-linux-gnu-gcc-7"
export CC_aarch64_unknown_linux_gnu="$arm64_cc"
export CXX_aarch64_unknown_linux_gnu="$arm64_cc --cxx"
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$arm64_cc"
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=--sysroot=$sysroot -C link-arg=-L$sysroot_lib"
if [[ "$host_arch" == x86_64 ]]; then
  # Archive tooling is object-format agnostic; the SDK's aarch64 binutils need
  # bundled libbfd on LD_LIBRARY_PATH, so prefer the host tools here.
  export AR_aarch64_unknown_linux_gnu=ar
  export RANLIB_aarch64_unknown_linux_gnu=ranlib
  export NM_aarch64_unknown_linux_gnu=nm
  export PKG_CONFIG_ALLOW_CROSS=1
  export PKG_CONFIG_SYSROOT_DIR="$sysroot"
  export PKG_CONFIG_LIBDIR="$sysroot/usr/lib/aarch64-linux-gnu/pkgconfig:$sysroot/usr/share/pkgconfig"
fi
mkdir -p "$cargo_home" "$target_dir" "$output_dir"
cat > "$cargo_home/config.toml" <<EOF
[source.crates-io]
replace-with = "vendored-sources"
[source.vendored-sources]
directory = "$vendor_root"
[net]
offline = true
EOF

cd "$repo_root"
echo "[wunder-cli-linux-arm64] cargo: $(cargo --version)"
cargo "${WUNDER_VENDORED_CONFIG[@]}" build --locked --offline --release -p wunder-cli --bin wunder-cli \
  "${cargo_target_args[@]}" -j "${WUNDER_BUILD_JOBS:-2}"

if [[ "$host_arch" == x86_64 ]]; then
  binary="$target_dir/$target/release/wunder-cli"
else
  binary="$target_dir/release/wunder-cli"
fi
require_file "$binary"
version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$repo_root/Cargo.toml" | head -n 1)"
[[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z.+-]*$ ]] || fail "could not read a safe workspace version"
release_binary="$output_dir/wunder-cli-$version-linux-arm64"
cp -f "$binary" "$release_binary"
"${strip_cmd[@]}" --strip-all --strip-unneeded "$release_binary"
# The SDK image ships mawk, whose regex lacks [[:space:]]; trim with sed.
machine="$(LC_ALL=C readelf -h "$release_binary" | LC_ALL=C awk -F: '/Machine:/{print $2}' | LC_ALL=C sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
[[ "$machine" == AArch64 ]] || fail "expected AArch64 ELF, got: ${machine:-unknown}"
required_glibc="$(LC_ALL=C readelf --version-info "$release_binary" | LC_ALL=C grep -oE 'GLIBC_[0-9.]+' | LC_ALL=C sed 's/GLIBC_//' | LC_ALL=C sort -Vu | LC_ALL=C tail -n 1 || true)"
[[ -n "$required_glibc" ]] || fail "could not determine executable GLIBC requirement"
[[ "$(printf '%s\n%s\n' "$max_glibc" "$required_glibc" | LC_ALL=C sort -V | tail -n 1)" == "$max_glibc" ]] || fail "GLIBC_$required_glibc exceeds release limit GLIBC_$max_glibc"
echo "[wunder-cli-linux-arm64] produced: $release_binary (GLIBC_$required_glibc)"
