#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
[[ -f .env ]] || cp .env.example .env
env_value() { sed -n "s/^$1=//p" .env | tail -n 1; }
quote_trim() {
    case "$1" in
        \"*\") printf '%s' "${1:1:${#1}-2}" ;;
        \'*\') printf '%s' "${1:1:${#1}-2}" ;;
        *) printf '%s' "$1" ;;
    esac
}

platform="$(env_value CI_PLATFORM)"; platform="${platform:-linux/arm64}"
case "$platform" in
    linux/arm64) expected_arch=aarch64; image_arch=arm64 ;;
    linux/amd64) expected_arch=x86_64; image_arch=amd64 ;;
    *) echo "未知的 CI_PLATFORM: $platform" >&2; exit 2 ;;
esac
[[ "$(uname -m)" == "$expected_arch" ]] || {
    echo "该套件面向 $platform，当前宿主是 $(uname -m)。" >&2
    echo '请在同架构的机器上用 prepare-bundle.py --arch 重新导出。' >&2
    exit 2
}

command -v docker >/dev/null || { echo 'Docker Engine 是必需的。' >&2; exit 2; }
# 两种 Compose 都能起：老机器上常只有 Python 版 docker-compose 1.29.x，
# 那种情况下用同目录的 compose.v1.yaml（见脚本末尾提示）。
if docker compose version >/dev/null 2>&1; then
    compose_flavor=v2
elif docker-compose version >/dev/null 2>&1; then
    compose_flavor=v1
else
    echo '需要 Docker Compose（v2 的 docker compose 或 v1 的 docker-compose）。' >&2
    exit 2
fi

# 镜像与 daemon 的版本下限都来自 .env：老 Runner（gitea/act_runner:0.2.11）的内置客户端
# 最低支持 API 1.24，新版 docker.gitea.com/runner:5.2.3 则要 1.40。低于下限时任务容器
# 会在启动阶段报 "client version ... is too new"，Gitea 本体却一切正常，很难猜。
api="$(docker version --format '{{.Server.APIVersion}}' 2>/dev/null || true)"
min_api="$(quote_trim "$(env_value RUNNER_MIN_DOCKER_API)")"; min_api="${min_api:-1.24}"
if [[ -n "$api" ]]; then
    to_number() { awk -F. '{ printf "%d%03d", $1, $2 }' <<< "$1"; }
    if (( $(to_number "$api") < $(to_number "$min_api") )); then
        echo "宿主 Docker API 是 $api，本套件的 Runner 至少需要 $min_api。" >&2
        echo '确认版本：docker version --format "{{.Server.APIVersion}}"' >&2
        echo '要么换成兼容老 daemon 的 Runner 镜像（.env 的 RUNNER_IMAGE），要么用 upgrade-docker.sh 升级引擎。' >&2
        exit 2
    fi
fi

sha256sum --check images/SHA256SUMS
for archive in images/*.tar.gz; do
    docker load --input "$archive"
done
# 载入后立即判架构：跨架构镜像在老 daemon 上往往要到任务容器启动才报错，那时已经晚了。
# 唯一的例外是 arm64 套件里的 Win7 安装包镜像：它是 amd64，由宿主 binfmt 模拟运行。
for image in $(sed -n 's/.*"image": "\([^"]*\)".*/\1/p' images/manifest.json 2>/dev/null | sort -u); do
    actual="$(docker image inspect --format '{{.Architecture}}' "$image" 2>/dev/null || true)"
    [[ -z "$actual" ]] && { echo "镜像未载入成功: $image" >&2; exit 2; }
    expected="$image_arch"
    if [[ "$image_arch" == arm64 && "$image" == wunder-offline-installer:* ]]; then
        expected=amd64
    fi
    [[ "$actual" == "$expected" ]] || {
        echo "镜像 $image 的架构是 $actual，套件要求 $expected。" >&2
        exit 2
    }
done

# ARM64 剖面带 binfmt/：注册 x86_64/i386 的 qemu 解释器，让安装包任务能 docker run amd64 镜像。
if [[ -d binfmt ]]; then
    bash binfmt/register.sh
    installer_image="$(quote_trim "$(env_value INSTALLER_IMAGE)")"
    if [[ -n "$installer_image" ]]; then
        emulated_arch="$(docker run --rm --security-opt seccomp=unconfined \
            --entrypoint uname "$installer_image" -m 2>/dev/null || true)"
        [[ "$emulated_arch" == x86_64 ]] || {
            echo "安装包镜像无法在 ARM 上模拟运行（uname 返回 '${emulated_arch:-<空>}'，应为 x86_64）。" >&2
            echo '检查 binfmt 注册与 Docker 是否能执行 amd64 镜像：' >&2
            echo "  docker run --rm --security-opt seccomp=unconfined --entrypoint uname $installer_image -m" >&2
            exit 2
        }
        echo "Win7 安装包镜像模拟自检通过（uname: $emulated_arch）"
    fi
fi

bash prepare-sdk.sh

# 固定名字的网络与卷：compose.v1.yaml 用 external 引用它们，必须先建好；
# compose.yaml（v2）自带 name:，预建同样无害，只是 up 时不再出现「新建」提示。
prefix="$(quote_trim "$(env_value STACK_PREFIX)")"; prefix="${prefix:-offline-gitea}"
sdk="$(quote_trim "$(env_value WUNDER_SDK_PATH)")"
[[ -n "$sdk" ]] || sdk="$(quote_trim "$(env_value RCHO_SDK_PATH)")"   # 兼容旧套件的键名
[[ -n "$sdk" && -d "$sdk" ]] || { echo "SDK 目录不存在: ${sdk:-<未记录>}（prepare-sdk.sh 未成功？）" >&2; exit 2; }

if docker volume inspect "$prefix-sdk" >/dev/null 2>&1; then
    current="$(docker volume inspect --format '{{ index .Options "device" }}' "$prefix-sdk" 2>/dev/null || true)"
    if [[ "$current" != "$sdk" ]]; then
        echo "SDK 卷 $prefix-sdk 指向 ${current:-<未绑定>}，与 $sdk 不一致，正在重建。"
        echo '如果 Runner 或任务容器还在运行，先 docker compose down（或 docker-compose stop）。'
        docker volume rm "$prefix-sdk" >/dev/null
    fi
fi
docker volume inspect "$prefix-sdk" >/dev/null 2>&1 || \
    docker volume create --driver local --opt type=none --opt o=bind --opt device="$sdk" \
        "$prefix-sdk" >/dev/null

# 安卓工具链与 APK 内嵌资产：x86_64 剖面用它构建 arm64 APK；arm64 剖面没有这套工具链，
# 目录留空即可（工作流里的安卓任务按宿主架构自行跳过）。
android_path="$(quote_trim "$(env_value WUNDER_ANDROID_PATH)")"
android_path="${android_path:-$(pwd -P)/android}"
mkdir -p "$android_path"
if ! grep -q '^WUNDER_ANDROID_PATH=' .env 2>/dev/null; then
    printf '\nWUNDER_ANDROID_PATH=%s\n' "$android_path" >> .env
fi
if docker volume inspect "$prefix-android" >/dev/null 2>&1; then
    current="$(docker volume inspect --format '{{ index .Options "device" }}' "$prefix-android" 2>/dev/null || true)"
    if [[ "$current" != "$android_path" ]]; then
        echo "Android 卷 $prefix-android 指向 ${current:-<未绑定>}，与 $android_path 不一致，正在重建。"
        echo '如果 Runner 或任务容器还在运行，先 docker compose down（或 docker-compose stop）。'
        docker volume rm "$prefix-android" >/dev/null
    fi
fi
docker volume inspect "$prefix-android" >/dev/null 2>&1 || \
    docker volume create --driver local --opt type=none --opt o=bind --opt device="$android_path" \
        "$prefix-android" >/dev/null
docker volume inspect "$prefix-build-cache" >/dev/null 2>&1 || \
    docker volume create "$prefix-build-cache" >/dev/null
docker network inspect "$prefix-ingress" >/dev/null 2>&1 || \
    docker network create "$prefix-ingress" >/dev/null
docker network inspect "$prefix-ci" >/dev/null 2>&1 || \
    docker network create --internal "$prefix-ci" >/dev/null

printf '%s\n' '镜像已载入、SDK 挂载就绪、网络与卷预建完成。'
printf '%s\n' '先在 .env 里把 GITEA_PUBLIC_URL 改成浏览器可访问的地址，然后按手上的 Compose 版本选一条：'
printf '%s\n' '  docker compose up -d                        # Compose v2'
printf '%s\n' '  docker-compose -f compose.v1.yaml up -d     # Compose v1'
[[ "$compose_flavor" == v2 ]] || printf '%s\n' '检测到本机只有 Compose v1，请用上面第二条命令。'
