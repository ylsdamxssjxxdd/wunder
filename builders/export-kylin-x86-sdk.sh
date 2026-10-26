#!/usr/bin/env bash
# Export the x86_64 Rust toolchains, host tools and metadata out of the
# wunder-rust-builder-kylin-x86 image into the kylin-x86 offline SDK. Run
# inside the image with the SDK output mounted at $OUTPUT_PROFILE:
#
#   docker run --rm --network none --platform linux/amd64 --entrypoint bash \
#     -e OUTPUT_PROFILE=/output -v /path/to/kylin-x86:/output \
#     -v /path/to/wunder/builders:/scripts:ro \
#     wunder-rust-builder-kylin-x86:ubuntu18 /scripts/export-kylin-x86-sdk.sh
#
# Mirrors builders/prepare-kylin-x86-sdk.sh for a native x86 host that has no
# kylin-arm source profile; sysroots, cargo vendor and AppImage runtimes are
# prepared separately on the host.
set -euo pipefail
out="${OUTPUT_PROFILE:?OUTPUT_PROFILE is required}"
[[ "$(uname -m)" == x86_64 ]] || { echo 'amd64 host required' >&2; exit 2; }
mkdir -p "$out/offline/rust/toolchains" "$out/metadata" "$out/offline/archives"

for toolchain in nightly-2026-03-14 1.92.0; do
  name="$toolchain-x86_64-unknown-linux-gnu"
  echo "[sdk] export $name"
  rsync -rlt --size-only "/root/.rustup/toolchains/$name/" "$out/offline/rust/toolchains/$name/"
  tar -C /root/.rustup/toolchains -cf "$out/offline/archives/$name.tar" "$name"
done

# Native and cross compilers plus all shared libraries remain available in the
# offline Docker image. This tar is a second, relocatable filesystem export.
echo '[sdk] export native and cross host tools'
tar -C / -cf "$out/offline/archives/host-tools-amd64-ubuntu18.tar" \
  usr/bin usr/lib usr/include usr/share usr/aarch64-linux-gnu usr/i686-w64-mingw32 lib lib64 bin etc/alternatives

rustup run nightly-2026-03-14 rustc -Vv > "$out/metadata/rustc-version.txt"
dpkg-query -W > "$out/metadata/packages.tsv"
cat > "$out/metadata/profile.json" <<'EOF'
{"schema":2,"profile":"kylin-x86","host":"x86_64-unknown-linux-gnu","build_os":"ubuntu:18.04","targets":["x86_64-unknown-linux-gnu","aarch64-unknown-linux-gnu","i686-win7-windows-gnu"],"toolchains":["1.92.0","nightly-2026-03-14"],"host_tools":"offline/archives/host-tools-amd64-ubuntu18.tar","image_archive":"offline/archives/builder-amd64-ubuntu18.docker.tar","validation":"metadata/smoke-test.txt"}
EOF
printf '[sdk] export completed\n'
