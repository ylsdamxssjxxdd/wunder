# Desktop Slint builders

本目录集中放置桌面 Slint 的发布、交叉编译和分发门禁脚本。`frontend-slint/scripts/`
只保留原生联调、启动回归和 AppImage 内容检查，不再维护第二套构建入口。

- `build-linux-arm64-appimage.*`：Ubuntu 18.04 ARM64 原生 AppImage。
- `build-linux-amd64-offline.*`：ARM64 Linux 主机交叉构建 Ubuntu 18.04 x86_64；传入 `-AppImage` 或 `--appimage` 时打包 AppImage。
- `build-win32-arm64-offline.*`：ARM64 Linux 主机交叉构建 Win32 i686 PE。
- `build-win7-offline.ps1` 与 `build-win7-slint.ps1`：Windows 主机的 Win7 x86 离线构建与 PE 门禁。
- `linux-amd64-cross-cc.sh`、`win7_host_tools.ps1`：目标链接器和宿主工具配置。

Win7 本机构建默认使用相邻的 `Rust-builder/win7/offline`；Linux 交叉构建（ARM64 Linux 主机或 Windows Docker）默认使用相邻的 `Rust-builder/kylin-arm/offline`。需要其它位置时显式设置
`WUNDER_BUILDER_ROOT` 或 PowerShell 的 `-BuilderRoot`；脚本不再探测或回退到旧 SDK 目录。
AppImage 必须通过 `WUNDER_APPIMAGE_RUNTIME` 或 `-AppImageRuntime` 提供同架构 type-2
runtime。打包只在目标输出目录创建唯一临时目录，成功后原子发布，失败时保留已有产物。
