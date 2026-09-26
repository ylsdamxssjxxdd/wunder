# Desktop and CLI builders

本目录集中放置桌面 Slint 与 CLI 的离线发布、交叉编译和分发门禁脚本。仓库根目录只保留两个公开入口：Linux 使用 `build.sh`，Windows 使用 `build.bat`。`frontend-slint/scripts/` 只保留原生联调、启动回归和 AppImage 内容检查。

公开入口统一以程序目标和分发架构选择构建：

```bash
# ARM64 Linux 主机；从其它 Linux 主机执行时在命令末尾添加 --docker。
bash build.sh -t desktop -a linux-arm64
bash build.sh -t cli -a linux-arm64
bash build.sh -t desktop -a linux-amd64
bash build.sh -t cli -a win7-x86

# linux-amd64 Desktop 默认是 ELF；仅明确要求时才打成 AppImage。
WUNDER_APPIMAGE_RUNTIME_AMD64=/path/to/runtime.AppImage \
  bash build.sh -t desktop -a linux-amd64 --appimage

# 构建当前 ARM64 Linux 主机可发布的全部程序：三个架构 × Desktop/CLI。
WUNDER_APPIMAGE_RUNTIME_ARM64=/path/to/arm64-runtime.AppImage \
WUNDER_APPIMAGE_RUNTIME_AMD64=/path/to/amd64-runtime.AppImage \
  bash build.sh -all
```

Windows 使用同一套目标/架构名称。Linux 目标通过 Docker 使用 `kylin-arm`；`win7-x86` 在本机通过 `win7` SDK 构建，在 ARM64 Linux 上也可交叉生成（同一 `i686-win7-windows-gnu` 目标，build-std 方式）。旧的 `win32-x86` 名称已并入 `win7-x86`（Win7 兼容构建覆盖全部 32 位 Windows 场景）：

```bat
build.bat -Target desktop -Arch win7-x86
build.bat -Target cli -Arch linux-amd64
build.bat -All -AppImageRuntimeArm64 X:\runtime-arm64.AppImage -AppImageRuntimeAmd64 X:\runtime-amd64.AppImage
```

`-all` 为完整发布构建 Linux ARM64、Linux amd64 与 Win7 x86 的 Desktop/CLI；Linux Desktop 产物均为 AppImage，CLI 在所有目标都只产生普通 ELF/EXE，绝不打包 AppImage。Windows 的 `-All` 默认分别使用相邻的 `Rust-builder/kylin-arm` 和 `Rust-builder/win7`；如需覆盖，使用 `-KylinBuilderRoot` 与 `-Win7BuilderRoot`，不要把两套 SDK 指到同一目录。

底层实现包括 `build-linux-*-*.sh`、`build-cli-*.sh`、`build-win7-*.ps1`、`build-win7-arm64-offline.sh`、`build-cli-win7-arm64-offline.sh` 和链接器辅助脚本。其中 `build-linux-appimage-native.sh` 是 CI 专用入口：在已预装 Ubuntu 18.04 工具链的容器内在线原生构建并打包 AppImage，不依赖 kylin-arm 离线 SDK，打包语义与 `build-linux-arm64-appimage.sh` 保持同步。Win7 Windows 本机构建固定使用相邻的 `Rust-builder/win7/offline`；Linux ARM64 开发机与 Windows Docker 的 Linux 交叉构建固定使用相邻的 `Rust-builder/kylin-arm/offline`（Win7 交叉使用其中的 `nightly-2026-03-14-aarch64-unknown-linux-gnu` 工具链 + rust-src，-Z build-std 重编 std，MinGW 链接与导入库由 `mingw-i686-windows-gnu` 提供，sdk-version 戳接受 wunder/rcho 两种印记）。Linux 可通过 `WUNDER_BUILDER_ROOT` 覆盖 kylin-arm 根目录；Windows 分别通过 `-KylinBuilderRoot`、`-Win7BuilderRoot` 覆盖两套 SDK。脚本不探测或回退到旧 SDK 目录。

`prepare-linux-amd64-runtime-sysroot.sh` 是 `kylin-arm` 的一次性维护脚本，用于补齐 Linux amd64 Desktop AppImage 的 X11、XTest 与 ALSA 运行库。它不是日常构建步骤，必须在 **x86_64 Ubuntu 18.04** 环境执行，且只能将 SDK 挂载为可写；脚本固定使用 Ubuntu Bionic 归档源，下载完成后会把校验清单写入 SDK。常规 `build.sh`、`build.bat` 构建始终离线，不会调用它。例如：

```bash
docker run --rm --network host \
  -v /path/to/kylin-arm:/builder/kylin-arm \
  -v /path/to/wunder:/workspace:ro \
  -w /workspace ubuntu:18.04 \
  env WUNDER_BUILDER_ROOT=/builder/kylin-arm \
  bash builders/prepare-linux-amd64-runtime-sysroot.sh
```

准备完成后，`linux-amd64-ubuntu18/root` 必须包含 `libX11.so.6`、`libXtst.so.6`、`libasound.so.2`、`libasound.so`、`libxcb.so.1`、`libxcb-xkb.so.1`、`libxkbcommon.so.0` 和 `libxkbcommon-x11.so.0`；其中 `libasound.so` 只用于交叉链接，AppImage 内容门禁验证其余运行库。
