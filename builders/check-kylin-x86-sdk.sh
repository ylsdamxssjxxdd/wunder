#!/usr/bin/env bash
set -euo pipefail
mkdir -p /sdk/metadata /tmp/sdk-smoke
test -s /sdk/offline/archives/builder-amd64-ubuntu18.docker.tar
exec > >(tee /sdk/metadata/smoke-test.txt) 2>&1
cd /tmp/sdk-smoke
printf 'int main(void) { return 0; }\n' > probe.c
printf 'fn main() { println!("ok"); }\n' > probe.rs
for cmd in gcc g++ aarch64-linux-gnu-gcc aarch64-linux-gnu-g++ i686-w64-mingw32-gcc i686-w64-mingw32-g++ windres cmake mksquashfs; do
  if [[ "$cmd" == windres ]]; then cmd=i686-w64-mingw32-windres; fi
  command -v "$cmd"
done
gcc probe.c -o native-c
./native-c
aarch64-linux-gnu-gcc probe.c -o arm64-c
i686-w64-mingw32-gcc probe.c -o win32-c.exe
readelf -h arm64-c | grep AArch64
i686-w64-mingw32-objdump -f win32-c.exe | grep pei-i386
rust=/sdk/offline/rust/toolchains/nightly-2026-03-14-x86_64-unknown-linux-gnu
"$rust/bin/rustc" probe.rs -o native-rust
./native-rust
"$rust/bin/rustc" --target aarch64-unknown-linux-gnu -C linker=aarch64-linux-gnu-gcc probe.rs -o arm64-rust
readelf -h /sdk/offline/appimage-runtime/runtime-x86_64 | grep X86-64
readelf -h /sdk/offline/appimage-runtime/runtime-aarch64 | grep AArch64
# Win7 tier-3 must compile std from local rust-src, not the modern pc target.
mkdir -p win7/src cargo-home
cp probe.rs win7/src/main.rs
printf '[package]\nname="sdk-probe"\nversion="0.1.0"\nedition="2021"\n' > win7/Cargo.toml
cat > cargo-home/config.toml <<'EOF'
[source.crates-io]
replace-with="vendored-sources"
[source.vendored-sources]
directory="/sdk/offline/cargo-vendor-slint"
[target.i686-win7-windows-gnu]
linker="i686-w64-mingw32-gcc"
[net]
offline=true
EOF
export PATH="$rust/bin:$PATH" RUSTC="$rust/bin/rustc" CARGO_HOME=/tmp/sdk-smoke/cargo-home
cargo -Z build-std=std,panic_abort build --offline --manifest-path win7/Cargo.toml --target i686-win7-windows-gnu
i686-w64-mingw32-objdump -f win7/target/i686-win7-windows-gnu/debug/sdk-probe.exe | grep pei-i386
imports="$(i686-w64-mingw32-objdump -p win7/target/i686-win7-windows-gnu/debug/sdk-probe.exe)"
if printf '%s' "$imports" | grep -Eiq 'combase.dll|api-ms-win-|ext-ms-win-|GetDpiForWindow|WaitOnAddress|WakeByAddress|SetThreadDescription'; then
  echo 'Win7 import gate failed'; exit 2
fi
printf 'PASS: offline native amd64, ARM64 and Win7 toolchain smoke tests\n'
