# Desktop and CLI builders

本目录集中放置桌面 Slint 与 CLI 的离线发布、交叉编译和分发门禁脚本。仓库根目录只保留两个公开入口：Linux 使用 `build.sh`，Windows 使用 `build.bat`。`frontend-slint/scripts/` 只保留原生联调、启动回归和 AppImage 内容检查。

公开入口统一以程序目标和分发架构选择构建：

```bash
# ARM64 Linux 主机；从其它 Linux 主机执行时在命令末尾添加 --docker。
bash build.sh -t desktop -a linux-arm64
bash build.sh -t cli -a linux-arm64
bash build.sh -t desktop -a linux-amd64
bash build.sh -t cli -a win32-x86

# linux-amd64 Desktop 默认是 ELF；仅明确要求时才打成 AppImage。
WUNDER_APPIMAGE_RUNTIME_AMD64=/path/to/runtime.AppImage \
  bash build.sh -t desktop -a linux-amd64 --appimage

# 构建当前 ARM64 Linux 主机可发布的全部程序：三个架构 × Desktop/CLI。
WUNDER_APPIMAGE_RUNTIME_ARM64=/path/to/arm64-runtime.AppImage \
WUNDER_APPIMAGE_RUNTIME_AMD64=/path/to/amd64-runtime.AppImage \
  bash build.sh -all
```

Windows 使用同一套目标/架构名称。Linux/Win32 目标通过 Docker 使用 `kylin-arm`，`win7-x86` 在本机通过 `win7` SDK 构建：

```bat
build.bat -Target desktop -Arch win7-x86
build.bat -Target cli -Arch linux-amd64
build.bat -Target cli -Arch win32-x86
build.bat -All -AppImageRuntimeArm64 X:\runtime-arm64.AppImage -AppImageRuntimeAmd64 X:\runtime-amd64.AppImage
```

`-all` 为完整发布构建 Linux ARM64、Linux amd64、Win32 x86 与 Win7 x86 的 Desktop/CLI；Linux Desktop 产物均为 AppImage，CLI 在所有目标都只产生普通 ELF/EXE，绝不打包 AppImage。Windows 的 `-All` 默认分别使用相邻的 `Rust-builder/kylin-arm` 和 `Rust-builder/win7`；如需覆盖，使用 `-KylinBuilderRoot` 与 `-Win7BuilderRoot`，不要把两套 SDK 指到同一目录。

底层实现包括 `build-linux-*-*.sh`、`build-cli-*.sh`、`build-win7-*.ps1` 和链接器辅助脚本。Win7 Windows 本机构建固定使用相邻的 `Rust-builder/win7/offline`；Linux ARM64 开发机与 Windows Docker 的 Linux/Win32 交叉构建固定使用相邻的 `Rust-builder/kylin-arm/offline`。Linux 可通过 `WUNDER_BUILDER_ROOT` 覆盖 kylin-arm 根目录；Windows 分别通过 `-KylinBuilderRoot`、`-Win7BuilderRoot` 覆盖两套 SDK。脚本不探测或回退到旧 SDK 目录。
