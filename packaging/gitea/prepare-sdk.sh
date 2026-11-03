#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
bundle="$(pwd -P)"
env_value() { sed -n "s/^$1=//p" .env | tail -n 1; }
quote_trim() {
    case "$1" in
        \"*\") printf '%s' "${1:1:${#1}-2}" ;;
        \'*\') printf '%s' "${1:1:${#1}-2}" ;;
        *) printf '%s' "$1" ;;
    esac
}

sdk="$(quote_trim "${WUNDER_SDK_PATH:-}")"
if [[ -z "$sdk" && -f .env ]]; then
    sdk="$(quote_trim "$(env_value WUNDER_SDK_PATH)")"
fi
sdk="${sdk:-$bundle/rust-builder/offline}"
[[ "$sdk" == /* ]] || { echo 'WUNDER_SDK_PATH must be an absolute host directory.' >&2; exit 2; }
if [[ ! -d "$sdk" && -d "$bundle/rust-builder/offline" ]]; then
    # 整套目录搬到别处后 .env 里记的还是旧绝对路径；套件自带 SDK 时直接改指本目录。
    printf '记录的 SDK 路径不存在：%s\n改用套件内的 %s\n' "$sdk" "$bundle/rust-builder/offline"
    sdk="$bundle/rust-builder/offline"
fi
if [[ ! -d "$sdk" && "$sdk" != "$bundle/rust-builder/offline" ]]; then
    echo 'The custom WUNDER_SDK_PATH directory does not exist.' >&2
    exit 2
fi

# 宿主架构决定 SDK 剖面：两套 SDK 只是宿主工具链互为镜像，
# 交叉目标（arm64、win7-x86）与 vendored crate 集完全相同。
profile="$(quote_trim "${SDK_PROFILE:-}")"
if [[ -z "$profile" && -f .env ]]; then
    profile="$(quote_trim "$(env_value SDK_PROFILE)")"
fi
if [[ -z "$profile" ]]; then
    [[ "$(uname -m)" != x86_64 ]] || profile=kylin-x86
    [[ -n "$profile" ]] || profile=kylin-arm
fi
case "$profile" in
    kylin-arm|kylin-x86) ;;
    *) echo "未知的 SDK_PROFILE: $profile" >&2; exit 2 ;;
esac

validate_sdk() {
    local directory="$1"
    local executable
    if [[ "$profile" == kylin-x86 ]]; then
        for executable in rust/toolchains/1.92.0-x86_64-unknown-linux-gnu/bin/rustc \
            rust/toolchains/1.92.0-x86_64-unknown-linux-gnu/bin/cargo \
            rust/toolchains/nightly-2026-03-14-x86_64-unknown-linux-gnu/bin/rustc \
            rust/toolchains/nightly-2026-03-14-x86_64-unknown-linux-gnu/bin/cargo \
            linux-amd64-ubuntu18/root/usr/bin/x86_64-linux-gnu-gcc-7 \
            linux-arm64-ubuntu18/root/usr/lib/gcc/aarch64-linux-gnu/7/cc1 \
            mingw-i686-windows-gnu/portable-bin/i686-w64-mingw32-gcc \
            mingw-i686-windows-gnu/portable-bin/i686-w64-mingw32-windres \
            mingw-i686-windows-gnu/portable-bin/i686-w64-mingw32-rust-gcc; do
            [[ -x "$directory/$executable" ]] || { echo "SDK executable is missing: $executable" >&2; return 2; }
        done
        # Win7 是 Tier 3 目标，必须能用本地 rust-src 重建 std，
        # 并用 SDK 里的 DW2 unwinder 链接（bionic mingw 自带的是 SJLJ）。
        [[ -d "$directory/rust/toolchains/nightly-2026-03-14-x86_64-unknown-linux-gnu/lib/rustlib/src/rust" ]] \
            || { echo 'SDK rust-src component is missing.' >&2; return 2; }
        [[ -f "$directory/mingw-i686-windows-gnu/rust-target/lib/libgcc_eh.a" ]] \
            || { echo 'SDK MinGW DW2 unwinder archive is missing.' >&2; return 2; }
        [[ "$(< "$directory/mingw-i686-windows-gnu/sdk-version")" == wunder-win32-x64-portable-v2 ]] \
            || { echo 'SDK MinGW kit is not the x86_64 portable layout.' >&2; return 2; }
        [[ -d "$directory/host-tools-amd64/root" ]] \
            || { echo 'SDK host-tools-amd64 is missing (arm64 cross linkers live there).' >&2; return 2; }
    else
        for executable in rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin/rustc \
            rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/bin/cargo \
            rust/toolchains/nightly-2026-03-14-aarch64-unknown-linux-gnu/bin/rustc \
            rust/toolchains/nightly-2026-03-14-aarch64-unknown-linux-gnu/bin/cargo \
            linux-arm64-ubuntu18/root/usr/lib/gcc/aarch64-linux-gnu/7/cc1 \
            linux-amd64-ubuntu18/root/usr/bin/x86_64-linux-gnu-gcc-7 \
            mingw-i686-windows-gnu/portable-bin/i686-w64-mingw32-gcc \
            mingw-i686-windows-gnu/portable-bin/i686-w64-mingw32-windres \
            mingw-i686-windows-gnu/portable-bin/i686-w64-mingw32-rust-gcc; do
            [[ -x "$directory/$executable" ]] || { echo "SDK executable is missing: $executable" >&2; return 2; }
        done
        # ARM 宿主的 win7 通道同为 Tier 3：build-std 要在 aarch64 nightly 里找到 rust-src，
        # MinGW 便携包必须是 aarch64 宿主版，PE 门禁用的是包内自带的 llvm-readobj。
        [[ -d "$directory/rust/toolchains/nightly-2026-03-14-aarch64-unknown-linux-gnu/lib/rustlib/src/rust" ]] \
            || { echo 'SDK rust-src component is missing (aarch64 nightly).' >&2; return 2; }
        # 稳定版工具链里要装 x86_64 目标 std：ARM 宿主交叉出 linux amd64 产物靠它，
        # 缺了会在 cargo 阶段报 can't find linker / target not installed。
        [[ -d "$directory/rust/toolchains/1.92.0-aarch64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/lib" ]] \
            || { echo 'SDK is missing the x86_64 target std inside the aarch64 toolchain.' >&2; return 2; }
        [[ -f "$directory/mingw-i686-windows-gnu/rust-target/lib/libgcc_eh.a" ]] \
            || { echo 'SDK MinGW DW2 unwinder archive is missing.' >&2; return 2; }
        [[ -x "$directory/mingw-i686-windows-gnu/llvm-tools/bin/llvm-readobj" ]] \
            || { echo 'SDK MinGW llvm-readobj is missing (win7 PE gate needs it).' >&2; return 2; }
        case "$(< "$directory/mingw-i686-windows-gnu/sdk-version")" in
            wunder-win32-arm64-portable-v2|rcho-win32-arm64-portable-v2) ;;
            *) echo 'SDK MinGW kit is not the aarch64 portable layout.' >&2; return 2 ;;
        esac
        # rcho 的只读视图要把 cargo-home-* 换成可写目录，SDK 里必须留着这些名字。
        [[ -d "$directory/cargo-home-slint" ]] \
            || { echo 'SDK is missing cargo-home-* directories (rcho SDK view needs them).' >&2; return 2; }
    fi
    [[ -d "$directory/cargo-vendor-slint" && -s "$directory/appimage-runtime/runtime-aarch64" \
        && -s "$directory/appimage-runtime/runtime-x86_64" ]] || { echo 'SDK is incomplete.' >&2; return 2; }
}

[[ -d "$sdk" ]] || {
    (cd rust-builder; sha256sum --check SHA256SUMS)
    staging="$(mktemp -d "$bundle/rust-builder/.extract.XXXXXXXX")"
    trap 'if [[ -n "${staging:-}" ]]; then rm -rf -- "$staging"; fi' EXIT
    tar -xzf rust-builder/offline-sdk.tar.gz -C "$staging" --no-same-owner
    [[ -d "$staging/offline" ]] || { echo 'SDK archive layout is invalid.' >&2; exit 2; }
    validate_sdk "$staging/offline"
    mv -- "$staging/offline" "$sdk"
    rmdir -- "$staging"
    staging=""
}
sdk="$(cd "$sdk" && pwd -P)"
validate_sdk "$sdk"

# 补充包是预制的：离线端只在发布时校验并复用，不在任务容器里编译 python/git。
prebuilt_archive="$bundle/rust-builder/supplement-prebuilt.tar.gz"
if [[ -f "$prebuilt_archive" && ! -d "$sdk/supplement-prebuilt" ]]; then
    printf '%s\n' 'Extracting the prebuilt supplement packs into the SDK.'
    tar -xzf "$prebuilt_archive" -C "$sdk" --no-same-owner
fi

[[ "$sdk" != *\'* && "$sdk" != *$'\n'* && "$sdk" != *$'\r'* ]] || {
    echo 'SDK mount paths must not contain single quotes or line breaks.' >&2; exit 2;
}
[[ -f .env ]] || cp .env.example .env
environment="$(mktemp "$bundle/.env.XXXXXXXX")"
trap 'rm -f -- "$environment"' EXIT
sed -e '/^WUNDER_SDK_PATH=/d' -e '/^SDK_PROFILE=/d' .env > "$environment"
printf "SDK_PROFILE='%s'\n" "$profile" >> "$environment"
printf "\nWUNDER_SDK_PATH='%s'\n" "$sdk" >> "$environment"
mv -f -- "$environment" .env
trap - EXIT

# 整套目录搬家后 SDK 绝对路径变了，而 compose 不会更新已存在卷的 device，
# 任务容器会挂到旧路径（通常已不存在）。这里按 .env 的前缀比对并重建该绑定卷。
prefix="$(quote_trim "$(env_value STACK_PREFIX)")"; prefix="${prefix:-offline-gitea}"
volume="$prefix-sdk"
if docker volume inspect "$volume" >/dev/null 2>&1; then
    current="$(docker volume inspect --format '{{ index .Options "device" }}' "$volume" 2>/dev/null || true)"
    if [[ "$current" != "$sdk" ]]; then
        printf 'SDK 卷 %s 指向 %s，与新路径 %s 不一致，正在重建（容器需先 docker compose down）。' \
            "$volume" "${current:-<未绑定>}" "$sdk"
        docker volume rm "$volume" >/dev/null
    fi
fi

printf '%s\n' "SDK prepared ($profile). Runner and build containers use the same read-only host directory."
