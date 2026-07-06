# wunder-cli Win7 GNU 构建 SOP

本文记录 Win7 兼容版 `wunder-cli.exe` 的固定构建流程。CLI 构建复用桌面端 Electron Win7 GNU 工具链配置，不另起一套 MinGW、Rust nightly、隔离 lockfile 或 target 目录规则。

## 目标

- 默认产物：Win7 32 位 CLI，`target=i686-win7-windows-gnu`。
- 默认工具链：读取 `desktop/electron/scripts/win7-gnu-toolchain.json`。
- 默认实验目录：`temp_dir/win7-gnu-lab/`。
- 最终交付目录：`temp_dir/win7-gnu-lab/cli-win7-ia32/dist/wunder-cli.exe`。

## 首次检查

```powershell
npm run doctor:cli:win7:gnu
```

该命令会检查：

- Rust/Cargo/Rustup 是否可用。
- 桌面端 Win7 GNU profile 是否存在。
- `C:\mingw32\bin` 中的 GNU 工具链是否齐全。
- 隔离 lockfile 与最终输出目录位置。

## 正式构建

```powershell
npm run build:cli:win7:gnu
```

等价底层命令：

```powershell
powershell -ExecutionPolicy Bypass -File crates/wunder-cli/scripts/build-win7-gnu.ps1 -Arch ia32
```

脚本会执行：

1. 复用 `desktop/electron/scripts/win7-gnu.common.ps1` 初始化 Win7 GNU 环境。
2. 使用隔离 lockfile：`temp_dir/win7-gnu-lab/cargo-win7.lock`。
3. 使用 release 构建和 `-Zbuild-std=std,panic_abort`。
4. 以 `-j 8` 限制 Rust 编译并发。
5. 默认对产物执行 `strip`，去除 COFF 符号，降低分发体积。
6. 运行 PE/DLL 静态检查，阻止 `api-ms-*` 或 `winrt` 导入进入 Win7 产物。
7. 运行 `wunder-cli.exe --help` 冒烟测试。
8. 复制产物到 `temp_dir/win7-gnu-lab/cli-win7-ia32/dist/` 并输出 SHA256。

## 快速重建

工具链和 cargo 缓存已初始化后：

```powershell
npm run build:cli:win7:gnu:fast
```

等价于传入 `-SkipBootstrap`，会跳过 rustup 与 fetch 初始化。

## 常用参数

```powershell
# 只诊断，不构建
powershell -ExecutionPolicy Bypass -File crates/wunder-cli/scripts/build-win7-gnu.ps1 -Arch ia32 -Doctor

# 构建但不 strip，便于排查符号
powershell -ExecutionPolicy Bypass -File crates/wunder-cli/scripts/build-win7-gnu.ps1 -Arch ia32 -NoStrip

# 跳过 help 冒烟测试
powershell -ExecutionPolicy Bypass -File crates/wunder-cli/scripts/build-win7-gnu.ps1 -Arch ia32 -SkipSmoke

# 显式构建 x64 实验产物
powershell -ExecutionPolicy Bypass -File crates/wunder-cli/scripts/build-win7-gnu.ps1 -Arch x64
```

## 兼容性要点

- `wunder-cli` 不应依赖 `wunder-desktop`、`tauri`、`wry`、Electron 或 webview。
- Win7 target 下 `reqwest` 必须关闭默认特性并使用 `rustls-no-provider`；workspace 的 `tokio-rustls` 必须显式使用 `ring` provider，避免默认 aws-lc provider 增大体积或引入不必要链路。
- Win7 CLI 不链接 `syntect` 默认语法库，TUI 代码块在 Win7 产物中回退为普通 Markdown 代码块渲染；普通 Windows/Linux/macOS CLI 仍保留高亮。
- Win7 legacy console 不支持 bracketed paste；TUI 必须把 `EnableBracketedPaste` / `DisableBracketedPaste` 作为 best-effort 能力探测，失败时回退到 Ctrl+V/Shift+Insert 显式粘贴路径，不能阻塞启动或退出。
- Win7 legacy console 使用 crossterm WinAPI backend 时不支持 `SetUnderlineColor`；workspace `ratatui` 必须显式关闭 default features，只开启 `crossterm` 和 `unstable-rendered-line-info`，避免每帧 reset underline color 时退出。
- 产物检查必须确认没有 `api-ms-*` 与 `winrt` 导入。
- GNU release 未 strip 时可能保留 `HAS_SYMS/HAS_LOCALS`，体积会明显偏大；正式分发默认 strip。
- 当前 32 位 Win7 CLI 仍静态链接完整 runtime，strip 后约 80 MiB。要进入 30 MiB 内，需要另拆 `cli-lite`/runtime feature，不能只靠 strip、TLS provider 或 UI 依赖裁剪达成。

## 手工复查命令

```powershell
$exe = "temp_dir\win7-gnu-lab\cli-win7-ia32\dist\wunder-cli.exe"
& "C:\mingw32\bin\objdump.exe" -f $exe
& "C:\mingw32\bin\objdump.exe" -p $exe | Select-String -Pattern "DLL Name|api-ms|winrt"
Get-FileHash -Algorithm SHA256 $exe
```

正常 ia32 产物应显示 `file format pei-i386`，且不应出现 `api-ms` 或 `winrt`。
