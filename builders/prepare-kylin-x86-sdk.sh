#!/usr/bin/env bash
# Run inside the pinned amd64 SDK image. The source profile stays read-only.
set -euo pipefail
src="${SOURCE_PROFILE:?SOURCE_PROFILE is required}"
out="${OUTPUT_PROFILE:?OUTPUT_PROFILE is required}"
[[ $(uname -m) == x86_64 ]] || { echo 'amd64 host required' >&2; exit 2; }
mkdir -p "$out/offline/rust/toolchains" "$out/metadata" "$out/offline/archives"
for name in cargo-vendor-slint; do
  echo "[sdk] sync $name"
  rsync -rlt --size-only "$src/offline/$name/" "$out/offline/$name/"
done
for toolchain in nightly-2026-03-14 1.92.0; do
  name="$toolchain-x86_64-unknown-linux-gnu"
  echo "[sdk] export $name"
  rsync -rlt --size-only "/root/.rustup/toolchains/$name/" "$out/offline/rust/toolchains/$name/"
  tar -C /root/.rustup/toolchains -cf "$out/offline/archives/$name.tar" "$name"
done
# Cargo configurations are created relative to the mounted SDK by build scripts;
# do not copy ARM-host rustup executables or absolute cargo-home paths.
# Archive Linux links/modes intact even when the destination is a Windows drive.
for name in linux-arm64-ubuntu18 linux-amd64-ubuntu18; do
  echo "[sdk] archive target sysroot $name"
  tar -C "$src/offline" -cf "$out/offline/archives/$name.tar" "$name"
done
mkdir -p "$out/offline/appimage-runtime"
# Remove only the known obsolete placeholder before installing real runtimes.
if [[ -d "$out/offline/appimage-runtime/runtime-x86_64" ]]; then
  rm "$out/offline/appimage-runtime/runtime-x86_64/README.txt"
  rmdir "$out/offline/appimage-runtime/runtime-x86_64"
fi

for arch in x86_64 aarch64; do
  cp "$src/offline/appimage-runtime/runtime-$arch" "$out/offline/appimage-runtime/runtime-$arch"
  readelf -h "$out/offline/appimage-runtime/runtime-$arch" > "$out/metadata/runtime-$arch.txt"
done
# Native and cross compilers plus all shared libraries remain available in the
# offline Docker image. This tar is a second, relocatable filesystem export.
echo '[sdk] export native and cross host tools'
tar -C / -cf "$out/offline/archives/host-tools-amd64-ubuntu18.tar" usr/bin usr/lib usr/include usr/share usr/aarch64-linux-gnu usr/i686-w64-mingw32 lib lib64 bin etc/alternatives
rustup run nightly-2026-03-14 rustc -Vv > "$out/metadata/rustc-version.txt"
dpkg-query -W > "$out/metadata/packages.tsv"
cat > "$out/metadata/profile.json" <<'EOF'
{"schema":2,"profile":"kylin-x86","host":"x86_64-unknown-linux-gnu","build_os":"ubuntu:18.04","targets":["x86_64-unknown-linux-gnu","aarch64-unknown-linux-gnu","i686-win7-windows-gnu"],"toolchains":["1.92.0","nightly-2026-03-14"],"host_tools":"offline/archives/host-tools-amd64-ubuntu18.tar","image_archive":"offline/archives/builder-amd64-ubuntu18.docker.tar","validation":"metadata/smoke-test.txt"}
EOF
printf '[sdk] export completed\n'
