# wunder-desktop Win7 Electron 构建方案

## 0. 目标

- 目标：在不污染仓库根 `node_modules/`、`.cargo/`、`target/` 的前提下，构建一版可用于 Windows 7 验证的 Electron 安装包。
- 范围：仅覆盖桌面安装包与构建链，不承诺自动更新、长期安全更新与生产支持。
- 当前结论：普通 Win7 试构建链可保留用于对比，但当前更推荐的固化路径是 **Electron 22 + x64 GNU bridge**。

## 1. 推荐路径

### 推荐一：Win7 GNU x64（当前主推）

- Electron 版本固定为 `22.3.27`。
- Rust bridge 使用 `x86_64-win7-windows-gnu` 构建，通过 nightly `-Zbuild-std` 生成 Win7 兼容版 Rust `std`。
- 打包脚本：`desktop/electron/scripts/build-win7-gnu.ps1`
- 根脚本入口：`npm run build:desktop:win7:gnu:x64`
- 产物目录：`temp_dir/win7-gnu-lab/electron-win7-x64/dist/`

### 备选二：Win7 MSVC ia32/x64

- 保留 `desktop/electron/scripts/build-win7.ps1` 作为兼容性对比链路。
- 适合继续验证 Win7 32 位或对比 GNU / MSVC 的行为差异。
- 产物目录：`temp_dir/win7-lab/electron-win7-<arch>/dist/`

## 2. 固化后的构建入口

### 根目录一键构建（推荐）

```powershell
npm run setup:desktop:win7:gnu:x64
npm run build:desktop:win7:gnu:x64
```

如果已经完成首次初始化，希望直接快速重建：

```powershell
npm run build:desktop:win7:gnu:x64:fast
```

如果想尝试静态 CRT：

```powershell
npm run build:desktop:win7:gnu:x64:static
```

### 直接执行 PowerShell 脚本

```powershell
powershell -ExecutionPolicy Bypass -File desktop/electron/scripts/build-win7-gnu.ps1 -Arch x64
```

## 3. 隔离目录

GNU 固化方案的所有缓存与产物统一落到：`temp_dir/win7-gnu-lab/`

- Cargo home：`temp_dir/win7-gnu-lab/cargo-home-win7-target`
- bridge target：`temp_dir/win7-gnu-lab/bridge-build-target-x64-win7-gnu`
- staging app：`temp_dir/win7-gnu-lab/electron-win7-x64/app`
- 安装包输出：`temp_dir/win7-gnu-lab/electron-win7-x64/dist`
- npm / Electron / electron-builder 缓存：同目录下独立缓存子目录

## 4. 已验证结果

已在当前机器上验证通过：

- `x86_64-win7-windows-gnu` bridge 可成功构建
- Electron 22 x64 壳可成功打出 NSIS 安装包
- 新 bridge 已移除 `GetSystemTimePreciseAsFileTime` 的静态导入
- 新 bridge 已移除 `api-ms-win-core-winrt-error-l1-1-0.dll` / `RoOriginateErrorW` 依赖链
- 产物示例：`temp_dir/win7-gnu-lab/electron-win7-x64/dist/wunder-desktop-win7-0.1.0-x64-setup.exe`

## 5. 关键实现点

- `desktop/electron/scripts/build-win7-gnu.ps1`：先单独构建 GNU bridge，再复用现有 Electron Win7 壳打包。
- `desktop/electron/scripts/setup-win7-gnu-toolchain.ps1`：首次初始化 Win7 GNU 工具链与缓存，产出可复用的工具链快照。
- `desktop/electron/scripts/win7-gnu-toolchain.json`：统一记录 Win7 GNU 的 Rust nightly、Electron 版本、目标三元组与 MinGW 路径。
- `patches/win7/tokio-rustls-0.26.4-win7/`：仅在 Win7 GNU 构建时临时注入，避免 `tokio-rustls` 默认拉入 `aws-lc-sys`。
- `desktop/electron/scripts/build-win7.ps1`：新增 `WUNDER_BRIDGE_BIN` 优先级，允许外部脚本注入 GNU 版 bridge。
- `desktop/electron/scripts/prepare-resources.js`：支持 `WUNDER_REPO_ROOT`、`WUNDER_SKIP_RUNTIME_DEPS_COPY` 与 `WUNDER_EXTRA_RUNTIME_FILES`，保证 staging 构建可复用、可扩展。
- `desktop/electron/electron-builder.win7.yml`：单独维护 Win7 Electron 22 的 builder 配置，不影响主线 Electron 38 配置。
- `temp_dir/win7-gnu-lab/toolchain-manifest.json`：每次初始化后输出当前工具链快照，便于后续快速重建与排查环境漂移。

## 6. 功能影响

以下影响仅针对 Win7 GNU 试包：

- 不再读取 Windows 系统代理设置，避免拉入 `windows-registry` / `winrt-error` 依赖链。
- HTTP/TLS 根证书改为 `rustls + webpki-roots`，公网证书通常可用，但企业私有 CA 或代理注入证书需额外验证。
- `tokio-rustls` 切到 `ring` 后端，常规功能预期不变，但底层密码库与主线不同。
- 自动更新继续保持关闭，不作为 Win7 验证范围。

## 7. 风险与边界

- GNU bridge 可以减少对 VC++ 运行库的依赖，但不等于完全没有系统 DLL 依赖。
- 当前 `x64` 路线可用；`i686-pc-windows-gnu` 在本机工具链上仍不稳定，不建议固化为默认方案。
- 自动更新仍默认关闭，避免 Win7 上的 TLS、签名与更新元数据带来额外不确定性。
- 最终是否可在 Win7 真机长期稳定运行，仍需验证 GPU、截图、权限申请、通知等桌面能力。
