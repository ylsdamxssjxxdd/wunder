#!/usr/bin/env bash
set -euo pipefail
offline_root="${WUNDER_OFFLINE_ROOT:-${WUNDER_BUILDER_ROOT:-$(cd -- "$(dirname -- "$0")/../../Rust-builder/kylin-arm" && pwd -P)}/offline}"
sdk="$offline_root/linux-arm64-ubuntu18"
root="$sdk/root"
debs="$sdk/debs/devel-arm64"
mode="${1:---prepare}"
case "$mode" in
  --prepare) mode=prepare ;;
  --repair) mode=repair ;;
  -h|--help)
    echo 'Usage: bash builders/prepare-linux-arm64-devel-sysroot.sh [--repair]'
    echo '  --repair  rewrite existing sysroot absolute links without apt or downloads'
    exit 0
    ;;
  *) echo "Unknown argument: $mode" >&2; exit 2 ;;
esac

if [[ "$mode" == prepare ]]; then
  mkdir -p "$debs" "$root"
  cat >/etc/apt/sources.list <<'EOF'
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic main universe multiverse restricted
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic-updates main universe multiverse restricted
deb [trusted=yes] http://ports.ubuntu.com/ubuntu-ports bionic-security main universe multiverse restricted
EOF
  printf 'Acquire::Check-Valid-Until "false";\n' >/etc/apt/apt.conf.d/99wunder-no-valid-until
  apt-get update -o Acquire::Check-Valid-Until=false
  cd "$debs"
  apt-get download libc6:arm64 libc6-dev:arm64 linux-libc-dev:arm64 libgcc1:arm64 libgcc-7-dev:arm64 cpp-7:arm64 gcc-7:arm64 binutils:arm64 libcc1-0:arm64 libgmp10:arm64 libisl19:arm64 libmpc3:arm64 libmpfr6:arm64 libstdc++6:arm64 zlib1g:arm64
fi

[[ -d "$root" ]] || { echo "ARM64 sysroot is missing: $root" >&2; exit 2; }
shopt -s nullglob
if [[ "$mode" == repair ]]; then
  cached_debs=(
    "$debs"/cpp-7_*_arm64.deb
    "$debs"/libgmp10_*_arm64.deb
    "$debs"/libisl19_*_arm64.deb
    "$debs"/libmpc3_*_arm64.deb
    "$debs"/libmpfr6_*_arm64.deb
  )
else
  cached_debs=("$debs"/*.deb)
fi
if ((${#cached_debs[@]})); then
  command -v dpkg-deb >/dev/null || { echo 'dpkg-deb is required to repair cached SDK packages' >&2; exit 2; }
  for deb in "${cached_debs[@]}"; do
    dpkg-deb -x "$deb" "$root"
  done
fi
# libgcc-7-dev supplies libgcc_s.so as an absolute /lib link. Convert every
# link whose target exists in the SDK so the standalone sysroot never reads a
# host library during linking.
while IFS= read -r -d '' link; do
  target="$(readlink "$link")"
  [[ "$target" == /* && -e "$root$target" ]] || continue
  relative_target="$(realpath --relative-to="$(dirname "$link")" "$root$target")"
  ln -sfn "$relative_target" "$link"
done < <(find "$root" -type l -print0)

gcc_lib="$root/usr/lib/gcc/aarch64-linux-gnu/7"
[[ -x "$gcc_lib/cc1" ]] \
  || { echo "ARM64 sysroot GCC backend is incomplete: $gcc_lib/cc1" >&2; exit 2; }
install_cc1_wrapper() {
  local backend="$gcc_lib/$1" real="$gcc_lib/$1.rcho-real"
  [[ -e "$backend" || -e "$real" ]] || return 0
  if [[ ! -f "$real" || ! -x "$real" ]]; then
    mv -f "$backend" "$real"
  elif ! grep -qF 'RCHO_GCC_CC1_WRAPPER=1' "$backend" 2>/dev/null; then
    mv -f "$backend" "$real"
  fi
  cat >"$backend" <<EOF
#!/usr/bin/env bash
# RCHO_GCC_CC1_WRAPPER=1
set -euo pipefail
backend_dir="\$(CDPATH= cd -- "\$(dirname -- "\$0")" && pwd -P)"
sysroot="\$(CDPATH= cd -- "\$backend_dir/../../../../.." && pwd -P)"
loader="\$sysroot/lib/ld-linux-aarch64.so.1"
[[ -x "\$loader" ]] || { echo "ARM64 sysroot dynamic loader is missing: \$loader" >&2; exit 127; }
exec "\$loader" --library-path "\$sysroot/usr/lib/aarch64-linux-gnu:\$sysroot/lib/aarch64-linux-gnu" "\$backend_dir/$1.rcho-real" "\$@"
EOF
  chmod 755 "$backend"
}
install_cc1_wrapper cc1
install_cc1_wrapper cc1plus
[[ -x "$gcc_lib/cc1.rcho-real" ]] \
  || { echo "ARM64 sysroot GCC backend payload is incomplete: $gcc_lib/cc1.rcho-real" >&2; exit 2; }
for library in libgmp.so.10 libisl.so.19 libmpc.so.3 libmpfr.so.6; do
  [[ -e "$root/usr/lib/aarch64-linux-gnu/$library" || -e "$root/lib/aarch64-linux-gnu/$library" ]] \
    || { echo "ARM64 sysroot GCC runtime is incomplete: $library" >&2; exit 2; }
done
[[ -e "$gcc_lib/libgcc_s.so" && -f "$root/lib/aarch64-linux-gnu/libgcc_s.so.1" ]] \
  || { echo "ARM64 sysroot libgcc is incomplete: $root" >&2; exit 2; }
if [[ "$mode" == prepare ]]; then
  echo "prepared ARM64 Ubuntu 18 development sysroot: $root"
else
  echo "repaired ARM64 Ubuntu 18 development sysroot: $root"
fi
