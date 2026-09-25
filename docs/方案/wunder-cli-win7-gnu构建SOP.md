# wunder-cli Win7 GNU 构建 SOP

本文记录 Win7 兼容版 `wunder-cli.exe` 的固定构建流程。构建入口使用根目录统一脚本，CLI 与 Slint Desktop 复用 `Rust-builder/win7` 的离线 Rust、MinGW 和 Cargo vendor。

## 目标

- 默认产物：Win7 32 位 CLI，`target=i686-win7-windows-gnu`。
- 默认工具链：相邻 `Rust-builder/win7/offline`。
- Cargo 构建缓存：`target/cli-win7-x86/cargo/`。
- 最终交付目录：`target/cli/dist/win7-x86/`。

## 首次检查

```powershell
.\build.bat -Target cli -Arch win7-x86 -Check
```

该命令会检查：

- `Rust-builder/win7/offline` 的 Rust、MinGW、Cargo vendor 是否齐全。
- CLI 完整运行时依赖是否包含在离线 vendor。
- 自定义 Win7 target 的 Rust、链接器和预编译 Windows import library 是否可用。

## 正式构建

```powershell
.\build.bat -Target cli -Arch win7-x86
```

脚本会执行：

1. 使用 release 构建和 `-Zbuild-std=std,panic_abort`。
2. 以 `-j 8` 限制 Rust 编译并发。
3. 默认对产物执行 `strip`，去除 COFF 符号，降低分发体积。
4. 运行 PE/DLL 静态检查，阻止 Win7 不支持的 DLL 与符号导入。
5. 复制产物到 `target/cli/dist/win7-x86/`。

## 快速重建

离线 SDK 和目标缓存可重用，重复执行同一命令即可增量构建：

```powershell
.\build.bat -Target cli -Arch win7-x86
```

## 与 builders/ 的关系

根目录仅保留 `build.sh` 和 `build.bat` 两个发布入口；具体实现位于 `builders/`。`builders/build-win7-cli.ps1` 是 Win7 CLI 的底层构建器，`builders/build-win7-offline.ps1` 是 Slint Desktop 的底层构建器。不要再使用旧的 Electron 构建脚本或 package.json 构建别名。

## 常用参数

```powershell
# 从非标准 SDK 目录构建
.\build.bat -Target cli -Arch win7-x86 -Win7BuilderRoot X:\Rust-builder\win7

# 构建 Desktop 与 CLI 的完整发布矩阵
.\build.bat -All -AppImageRuntimeArm64 X:\runtime-arm64.AppImage -AppImageRuntimeAmd64 X:\runtime-amd64.AppImage
```

## 兼容性要点

- `wunder-cli` 不应依赖 `wunder-desktop`、`tauri`、`wry`、Electron 或 webview。
- Win7 target 下 `reqwest` 必须关闭默认特性并使用 `rustls-no-provider`；workspace 的 `tokio-rustls` 必须显式使用 `ring` provider，避免默认 aws-lc provider 增大体积或引入不必要链路。
- Win7 CLI 不链接 `syntect` 默认语法库，TUI 代码块在 Win7 产物中回退为普通 Markdown 代码块渲染；普通 Windows/Linux/macOS CLI 仍保留高亮。
- Win7 legacy console 不支持 bracketed paste；TUI 必须把 `EnableBracketedPaste` / `DisableBracketedPaste` 作为 best-effort 能力探测，失败时回退到 Ctrl+V/Shift+Insert 显式粘贴路径，不能阻塞启动或退出。
- Win7 legacy console 使用 crossterm WinAPI backend 时不支持 `SetUnderlineColor`；workspace `ratatui` 必须显式关闭 default features，只开启 `crossterm` 和 `unstable-rendered-line-info`，避免每帧 reset underline color 时退出。
- 产物检查必须确认没有 `api-ms-*` 与 `winrt` 导入。
- GNU release 未 strip 时可能保留 `HAS_SYMS/HAS_LOCALS`，体积会明显偏大；正式分发默认 strip。
- 当前 32 位 Win7 CLI 仍静态链接完整 runtime，strip 后体积以当次构建输出为准（历史上约 80 MiB 量级）。要显著缩小体积，需要另拆 `cli-lite`/runtime feature，不能只靠 strip、TLS provider 或 UI 依赖裁剪达成。

## 手工复查命令

```powershell
$exe = "target\cli\dist\win7-x86\wunder-cli-<version>-win7-x86.exe"
& "..\Rust-builder\win7\offline\mingw32\bin\objdump.exe" -f $exe
& "..\Rust-builder\win7\offline\mingw32\bin\objdump.exe" -p $exe | Select-String -Pattern "DLL Name|api-ms|winrt"
Get-FileHash -Algorithm SHA256 $exe
```

正常 ia32 产物应显示 `file format pei-i386`，且不应出现 `api-ms` 或 `winrt`。
