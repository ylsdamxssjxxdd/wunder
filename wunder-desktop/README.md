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
npm --prefix frontend run build

# 3) 进入 Tauri 目录
Set-Location wunder-desktop

# 4) 可选：临时覆盖 bundle 配置，避免 updater 未配置时报错
@'
{
  "bundle": {
    "createUpdaterArtifacts": false
  }
}
'@ | Set-Content -Path tauri.bundle.tmp.json -Encoding UTF8

# 5) 打包（NSIS 安装包）
$env:CARGO_TARGET_DIR = "target"
$env:PATH = "C:\Program Files (x86)\NSIS;$env:PATH"
cargo tauri build -f desktop -c tauri.bundle.tmp.json --bundles nsis --no-sign -- --manifest-path ../Cargo.toml
```

## 产物路径

- 可执行文件：`wunder-desktop/target/release/wunder-desktop.exe`
- 安装包：`wunder-desktop/target/release/bundle/nsis/wunder-desktop_0.1.0_x64-setup.exe`

## 打包资源说明

- Tauri 安装包会携带以下运行资源：
  - `config/`
  - `prompts/`
  - `skills/`
  - `scripts/`
  - `frontend/dist/`
- 运行时会将内置 `skills/` 增量同步到本地工作区：`<userData>/WUNDER_WORK/skills`
  - 已存在的本地文件不会被覆盖，便于用户自行修改

## 常见问题

- 报错 `plugins > updater doesn't exist`
  - 使用上面的 `tauri.bundle.tmp.json`（关闭 `createUpdaterArtifacts`）重新打包。
- 报错找不到 `makensis`
  - 确认 NSIS 已安装，并将其目录加入当前会话 `PATH`。
- 首次打包较慢
  - 属于正常现象，Rust 依赖会进行完整编译。
