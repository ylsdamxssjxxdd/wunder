#!/usr/bin/env bash
# 生成 aarch64 宿主上的 x86_64 安卓工具链包装器（qemu-user 模拟）。
#
# NDK 与 build-tools 只有 x86_64 宿主版；aarch64 任务机把 qemu-x86_64-static 和一份
# 最小 x86_64 glibc 运行库放在 android 卷里，本脚本生成 bin/ 下的包装器：
#   - clang/ar 等只在 C 编译阶段使用，统一加 -fintegrated-cc1，避免 clang 再 exec 自身；
#   - 最终链接由 rust-linker 直接调 ld.lld 并补齐 Android crt/运行库；
#   - aapt2/zipalign 由 Gradle 直接调用，包装器只负责套 qemu。
set -euo pipefail

root="${1:?用法: android-host-tools.sh <android-root> [ndk-version]}"
ndk_version="${2:-28.2.13676358}"
llvm_rel="sdk/ndk/$ndk_version/toolchains/llvm/prebuilt/linux-x86_64"
build_tools_rel="sdk/build-tools/36.0.0"
qemu="$root/qemu/qemu-x86_64-static"
sysroot_root="$root/x86_64-root"

[[ -x "$qemu" ]] || { echo "缺少 $qemu" >&2; exit 2; }
[[ -d "$sysroot_root/lib64" ]] || { echo "缺少 $sysroot_root/lib64" >&2; exit 2; }
[[ -d "$root/$llvm_rel/bin" ]] || { echo "缺少 $root/$llvm_rel/bin" >&2; exit 2; }
[[ -x "$root/$build_tools_rel/aapt2" ]] || { echo "缺少 $root/$build_tools_rel/aapt2" >&2; exit 2; }

out="$root/bin"
mkdir -p "$out"

emit() {
    local name="$1" body="$2"
    printf '%s\n' '#!/bin/bash' 'set -euo pipefail' \
        'ROOT="$(cd -- "$(dirname -- "$0")/.." && pwd -P)"' "$body" > "$out/$name"
    chmod +x "$out/$name"
}

cat > "$out/_clang-wrap" <<EOF
#!/bin/bash
set -euo pipefail
ROOT="\$(cd -- "\$(dirname -- "\$0")/.." && pwd -P)"
LLVM="\$ROOT/$llvm_rel"
QEMU="\$ROOT/qemu/qemu-x86_64-static"
TARGET="\${WUNDER_CLANG_TARGET:-}"
COMPILER="\${WUNDER_CLANG_COMPILER:-clang}"
args=("\$@")
input="" out="" compile=0
for ((i = 0; i < \${#args[@]}; i++)); do
    case "\${args[i]}" in
        -c) compile=1 ;;
        -o) out="\${args[i + 1]:-}" ;;
        *.S|*.s) input="\${args[i]}" ;;
    esac
done
if [[ "\$compile" == 1 && -n "\$input" ]]; then
    # .S 走独立的 -cc1as 子进程；qemu 里无法 exec x86_64 子进程，
    # 改成先预处理、再用单进程 cc1as 汇编。
    tmp="\$(mktemp "\${TMPDIR:-/tmp}/wunder-asm.XXXXXX.i")"
    trap 'rm -f "\$tmp"' EXIT
    pre=()
    skip=0
    for ((i = 0; i < \${#args[@]}; i++)); do
        arg="\${args[i]}"
        [[ \$skip == 1 ]] && { skip=0; continue; }
        case "\$arg" in
            -c) pre+=(-E) ;;
            -o) skip=1 ;;
            -Wa,*) ;;
            *) pre+=("\$arg") ;;
        esac
    done
    pp_extra=()
    [[ -n "\$TARGET" ]] && pp_extra+=(--target="\$TARGET")
    "\$QEMU" -L "\$ROOT/x86_64-root" "\$LLVM/bin/\$COMPILER" -fintegrated-cc1 "\${pp_extra[@]}" "\${pre[@]}" -o "\$tmp"
    asm=(-cc1as -filetype obj)
    [[ -n "\$TARGET" ]] && asm=(-cc1as -triple "\$TARGET" -filetype obj)
    "\$QEMU" -L "\$ROOT/x86_64-root" "\$LLVM/bin/clang" "\${asm[@]}" "\$tmp" -o "\$out"
    exit 0
fi
extra=()
[[ -n "\$TARGET" ]] && extra+=(--target="\$TARGET")
exec "\$QEMU" -L "\$ROOT/x86_64-root" "\$LLVM/bin/\$COMPILER" -fintegrated-cc1 "\${extra[@]}" "\$@"
EOF
chmod +x "$out/_clang-wrap"

compiler() {
    local name="$1" target="$2"
    local env_target="" compiler="clang"
    [[ -n "$target" ]] && env_target="WUNDER_CLANG_TARGET=$target "
    [[ "$name" == *++* ]] && compiler="clang++"
    emit "$name" "${env_target}WUNDER_CLANG_COMPILER=$compiler exec \"\$(dirname -- \"\$0\")/_clang-wrap\" \"\$@\""
}

passthrough() {
    local name="$1" relative="$2"
    emit "$name" "exec \"\$ROOT/qemu/qemu-x86_64-static\" -L \"\$ROOT/x86_64-root\" \\
    \"\$ROOT/$relative\" \"\$@\""
}

compiler clang ""
compiler clang++ ""
compiler aarch64-linux-android26-clang aarch64-linux-android26
compiler aarch64-linux-android26-clang++ aarch64-linux-android26
passthrough llvm-ar "$llvm_rel/bin/llvm-ar"
passthrough llvm-ranlib "$llvm_rel/bin/llvm-ranlib"
passthrough llvm-strip "$llvm_rel/bin/llvm-strip"
passthrough llvm-objcopy "$llvm_rel/bin/llvm-objcopy"
passthrough ld.lld "$llvm_rel/bin/ld.lld"
passthrough aapt2 "$build_tools_rel/aapt2"
passthrough zipalign "$build_tools_rel/zipalign"

cat > "$out/rust-linker" <<EOF
#!/bin/bash
set -euo pipefail
ROOT="\$(cd -- "\$(dirname -- "\$0")/.." && pwd -P)"
LLVM="\$ROOT/$llvm_rel"
SYSROOT="\$LLVM/sysroot"
CLANGLIB="\$LLVM/lib/clang/19"
TARGET="\$SYSROOT/usr/lib/aarch64-linux-android/26"
parts=()
for arg in "\$@"; do
    case "\$arg" in
        -Wl,*) IFS=, read -r -a pieces <<<"\${arg#-Wl,}"; parts+=("\${pieces[@]}") ;;
        -nodefaultlibs) ;;
        *) parts+=("\$arg") ;;
    esac
done
exec "\$ROOT/qemu/qemu-x86_64-static" -L "\$ROOT/x86_64-root" "\$LLVM/bin/ld.lld" \\
    -EL --fix-cortex-a53-843419 -z now -z relro -z max-page-size=16384 \\
    --hash-style=gnu --eh-frame-hdr -m aarch64linux \\
    "\$TARGET/crtbegin_so.o" \\
    -L"\$TARGET" -L"\$SYSROOT/usr/lib/aarch64-linux-android" -L"\$SYSROOT/usr/lib" \\
    -L"\$CLANGLIB/lib/linux/aarch64" \\
    "\${parts[@]}" \\
    "\$CLANGLIB/lib/linux/libclang_rt.builtins-aarch64-android.a" -l:libunwind.a -ldl -lc \\
    "\$TARGET/crtend_so.o"
EOF
chmod +x "$out/rust-linker"

echo "[android-host-tools] 已生成: $(ls "$out" | tr '\n' ' ')"
