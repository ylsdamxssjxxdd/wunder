# wunder-desktop Win7 x86 部署方案（Tauri）

## 0. 目标与范围

- 目标：产出可在 Windows 7 32 位（x86）上运行的 `wunder-desktop` 安装包（NSIS）。
- 范围：仅覆盖构建与部署流程，不改动业务功能与 UI。
- 结论：Win7 上必须使用 **WebView2 固定运行时 109 x86**，并采用离线安装/固定运行时方式部署。

## 1. 关键限制

- **WebView2 版本限制**：Win7 只能运行 WebView2 109，且已停止更新，必须固定运行时版本。
- **系统 TLS 兼容性**：Win7 对现代 TLS 兼容性较差，在线下载 WebView2 容易失败，建议离线或固定运行时。
- **Rust 兼容性**：建议使用 **Rust 1.77.x** 构建 `i686-pc-windows-msvc`，或使用 Win7 专用 target（需要 build-std）。
- **官方支持范围**：Tauri 与 WebView2 均已不再官方支持 Win7，属于“可运行但非主流支持”。

## 2. 目录约定

- `desktop/tauri/webview2/win7-x86/`：放置解压后的 WebView2 Fixed Runtime 109 x86。
- `desktop/tauri/tauri.bundle.win7-x86.json`：Win7 x86 专用 bundle 覆盖配置。

## 3. 准备材料

- WebView2 Fixed Runtime 109 x86（解压到 `desktop/tauri/webview2/win7-x86/`）。
- NSIS 安装器（用于生成 `.exe` 安装包）。
- Rust 工具链与 Tauri CLI。
- Node.js（用于构建 `frontend/dist`）。

## 4. 配置方案

新增一个 Win7 x86 专用的 bundle 覆盖配置文件：

`desktop/tauri/tauri.bundle.win7-x86.json`

```json
{
  "bundle": {
    "targets": ["nsis"],
    "windows": {
      "webviewInstallMode": {
        "type": "fixedRuntime",
        "path": "./webview2/win7-x86/"
      }
    },
    "createUpdaterArtifacts": false
  }
}
```

说明：

- `fixedRuntime` 强制使用本地 WebView2 109。
- 关闭 `createUpdaterArtifacts` 可以避免未配置 updater 时打包报错。

## 5. 构建流程（Windows 构建机）

1. 构建 bridge（二进制保持与桌面版本一致）
```powershell
cargo build --release --bin wunder-desktop-bridge
```

2. 构建前端产物
```powershell
npm install --workspaces --include-workspace-root=false
npm run build --workspace wunder-frontend
```

3. 进入 Tauri 目录
```powershell
Set-Location desktop/tauri
```

4. 选择 Rust 工具链与目标

- 推荐方案（兼容 Win7）：
  - 使用 Rust `1.77.x`。
  - 目标：`i686-pc-windows-msvc`。
- 备选方案（更复杂）：
  - 使用 `i686-win7-windows-msvc` 目标。
  - 需要 `-Z build-std` 并配套 nightly 或自编译标准库。

5. 打包（示例：推荐方案）
```powershell
$env:CARGO_TARGET_DIR = "..\\..\\target\\win7-x86"
$env:PATH = "C:\Program Files (x86)\NSIS;$env:PATH"
cargo +1.77.2 tauri build -f desktop -c tauri.bundle.win7-x86.json --bundles nsis --no-sign --target i686-pc-windows-msvc -- --manifest-path ../../Cargo.toml
```

## 6. 产物路径

- 可执行文件：`target/win7-x86/i686-pc-windows-msvc/release/wunder-desktop.exe`
- 安装包：`target/win7-x86/i686-pc-windows-msvc/release/bundle/nsis/wunder-desktop_0.1.0_x86-setup.exe`

## 7. Win7 真机验证清单

- 安装包可正常启动，无 “WebView2 缺失” 错误。
- 应用首屏正常加载，基础聊天与配置页面可用。
- 资源目录可写（自动创建 `WUNDER_TEMPD` 与默认工作区）。

## 8. 风险与维护策略

- Win7 与 WebView2 109 已停止安全更新，生产环境需要隔离与风险评估。
- 在线更新在 Win7 上可能因 TLS 失败，建议：
  - 默认关闭自动更新。
  - 采用手工更新或内网分发。
- 后续如需维持 Win7 兼容，应锁定构建工具链版本，避免升级导致最低系统版本提升。


