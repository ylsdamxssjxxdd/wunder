# wunder-desktop (Tauri) 打包说明

本目录用于 `wunder-desktop`（Tauri 原生桌面端）构建与打包。

## 环境准备（Windows）

- Rust 工具链（含 `cargo`）
- Node.js（用于构建 `frontend/dist`）
- Tauri CLI
  - `cargo install tauri-cli --locked`
- NSIS（用于生成 `.exe` 安装包）
  - 默认路径：`C:\Program Files (x86)\NSIS\makensis.exe`

## 一次完整打包（Windows x64）

在仓库根目录执行：

```powershell
# 1) 构建后端桥接二进制（推荐与桌面包同步更新）
cargo build --release --bin wunder-desktop-bridge

# 2) 构建前端产物
npm install --workspaces --include-workspace-root=false
npm run build --workspace wunder-frontend

# 3) 进入 Tauri 目录
Set-Location desktop/tauri

# 4) 可选：临时覆盖 bundle 配置，避免 updater 未配置时报错
@'
{
  "bundle": {
    "createUpdaterArtifacts": false
  }
}
'@ | Set-Content -Path tauri.bundle.tmp.json -Encoding UTF8

# 5) 打包（NSIS 安装包）
$env:PATH = "C:\Program Files (x86)\NSIS;$env:PATH"
cargo tauri build -f desktop -c tauri.bundle.tmp.json --bundles nsis --no-sign -- --manifest-path ../../Cargo.toml
```

## 产物路径

- 可执行文件：`target/release/wunder-desktop.exe`
- 安装包：`target/release/bundle/nsis/wunder-desktop_0.2.0_x64-setup.exe`
- `desktop/tauri/.cargo/config.toml` 已固定将本目录下触发的 Cargo/Tauri 产物写入仓库根 `target/`

## Win7 x86 试构建（隔离实验）

为避免污染仓库根 `.cargo/` 与 `target/`，仓库提供了隔离实验脚本：

```powershell
powershell -ExecutionPolicy Bypass -File desktop/tauri/scripts/build-win7.ps1
```

- 默认使用 `temp_dir/win7-lab/cargo-home` 与 `temp_dir/win7-lab/tauri-build-target`
- 默认优先使用 `desktop/tauri/webview2/win7-x86/` 下的 Fixed Runtime 109；如果目录不存在，则自动退回 `skip` 模式，仅验证打包链是否可产出 NSIS 包
- 可用 `-WebviewMode fixed` 强制要求 Fixed Runtime，或 `-WebviewMode skip` 强制跳过 WebView2 打包
- 默认产物目录：`temp_dir/win7-lab/tauri-build-target/i686-pc-windows-msvc/release/bundle/nsis/`

## Win7 GNU 试构建（减少 MSVC 依赖）

如果想尝试 `windows-gnu` 目标，可执行：

```powershell
# 推荐先试 x64 GNU
powershell -ExecutionPolicy Bypass -File desktop/tauri/scripts/build-win7-gnu.ps1 -Arch x64

# 若继续试 32 位 GNU
powershell -ExecutionPolicy Bypass -File desktop/tauri/scripts/build-win7-gnu.ps1 -Arch ia32
```

- 脚本会把 `CARGO_HOME` 与 `CARGO_TARGET_DIR` 都隔离到 `temp_dir/win7-gnu-lab/`
- GNU 试构建默认优先用 `skip-webview`，避免在 WebView2 运行时未就绪时把问题混在一起
- 默认产物目录：`temp_dir/win7-gnu-lab/tauri-build-target-<arch>-gnu/<target>/release/bundle/nsis/`

## 打包资源说明

- Tauri 安装包会携带以下运行资源：
  - `config/`
  - `scripts/`
  - `frontend/dist/`
- 运行时会将内置 `config/skills/` 同步到本地工作区：`<userData>/WUNDER_WORK/skills`
  - 同名内置技能会被覆盖为最新打包版本（用于升级后保持一致）
  - 用户上传的自定义技能请放在 `admin_skills` 或 `user_tools/<user>/skills`，不会被内置同步覆盖

## 常见问题

- 报错 `plugins > updater doesn't exist`
  - 使用上面的 `tauri.bundle.tmp.json`（关闭 `createUpdaterArtifacts`）重新打包。
- 报错找不到 `makensis`
  - 确认 NSIS 已安装，并将其目录加入当前会话 `PATH`。
- 首次打包较慢
  - 属于正常现象，Rust 依赖会进行完整编译。
- JavaScript 依赖统一安装在仓库根目录 `node_modules/`
  - `desktop/tauri` 本身不维护单独的 `node_modules/`。

