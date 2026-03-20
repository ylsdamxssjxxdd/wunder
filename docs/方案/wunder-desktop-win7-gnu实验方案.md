# wunder-desktop Win7 GNU 固化方案

## 0. 定位

- 快速发布与日常重建请优先参考：`docs/方案/wunder-desktop-win7-发布SOP.md`
- 目标：将 **Electron 22 + x64 GNU bridge** 固化为当前仓库里 Win7 方向的首选构建方式。
- 原则：不污染仓库根 `.cargo/`、`target/`、`node_modules/`，所有 GNU 实验与正式构建产物统一落到 `temp_dir/win7-gnu-lab/`。
- 当前主线：以 `x86_64-win7-windows-gnu` 为 bridge 目标三元组，通过 nightly `-Zbuild-std=std,panic_abort` 构建 Win7 兼容版 Rust `std`。

## 1. 当前结论

- `x86_64-win7-windows-gnu` 的 `wunder-desktop-bridge` 已成功构建并完成 Electron 22 x64 安装包产出。
- 当前 Win7 方向的默认正式出包流程，固定为 `npm run build:desktop:win7:gnu:x64`。
- 当前 Win7 方向的最终交付物，固定为 **安装包 + 补充包 zip** 两个文件；安装包本体不再内置 Python/Git。
- 新 bridge 已移除对 `GetSystemTimePreciseAsFileTime` 的导入，改走 Win7 可用的时间 API 路径。
- 新 bridge 已移除 `api-ms-win-core-winrt-error-l1-1-0.dll` / `RoOriginateErrorW` 相关依赖链。
- DPI 感知初始化改为运行时按需加载，避免静态引入 `shcore` 的 Win8+ 入口点。
- `i686-win7-windows-gnu` 当前仍受本机 `C:\mingw32` 工具链限制，尚不适合作为固化方案。
- Tauri GNU 当前仍阻塞在 `resource.lib` 与 GNU 链接器的资源编译兼容问题，因此暂不作为固化路径。

## 2. 推荐构建命令

### 默认发布入口

```powershell
npm run setup:desktop:win7:gnu:x64
npm run build:desktop:win7:gnu:x64
```

- 以后 Win7 版本默认优先走这条链路。
- 该入口会同时完成：Win7 GNU bridge 构建、Win7 `common` Python/Git 补充运行时构建、Electron NSIS 安装包产出。
- 该入口会把补充包 zip 复制到安装包输出目录旁边，但不会把 Python/Git 并入安装包。
- Win7 `common` 补充包默认使用清华 Tuna 简单索引安装 `packaging/python/requirements-win7-common.txt` 中的依赖。

### 快速重建入口

```powershell
npm run build:desktop:win7:gnu:x64:fast
```

### 同时产出补充包

```powershell
npm run build:desktop:win7:gnu:x64

# 如需显式切到 minimal 补充包
npm run build:desktop:win7:gnu:x64:release:minimal
```

### 静态 CRT 试探入口

```powershell
npm run build:desktop:win7:gnu:x64:static
```

### 脚本直跑

```powershell
powershell -ExecutionPolicy Bypass -File desktop/electron/scripts/build-win7-gnu.ps1 -Arch x64
```

## 3. 目录与产物

- Cargo home：`temp_dir/win7-gnu-lab/cargo-home-win7-target`
- bridge target：`temp_dir/win7-gnu-lab/bridge-build-target-x64-win7-gnu`
- Electron staging：`temp_dir/win7-gnu-lab/electron-win7-x64/app`
- 最终交付安装包：`temp_dir/win7-gnu-lab/electron-win7-x64/dist/wunder-desktop-win7-0.1.0-x64-setup.exe`
- 同目录补充包：`temp_dir/win7-gnu-lab/electron-win7-x64/dist/wunder补充包-win7-x64-common.zip`
- Win7 专用 patch 配置：`temp_dir/win7-gnu-lab/cargo-win7-patch.toml`
- 工具链快照：`temp_dir/win7-gnu-lab/toolchain-manifest.json`

其中，补充包 zip 用于按需补齐 Python/Git；默认对外分发时，建议与安装包一起提供，但不再嵌入安装包。

## 4. 固化点

- `desktop/electron/scripts/win7-gnu-toolchain.json`：统一固化 Win7 GNU 目标、nightly 工具链、Electron 版本与 MinGW 路径。
- `desktop/electron/scripts/setup-win7-gnu-toolchain.ps1`：首次初始化入口，负责检查 MinGW、安装 Rust nightly、预拉取 Cargo 依赖并生成工具链快照。
- `desktop/electron/scripts/build-win7-gnu.ps1`：Win7 GNU bridge + Electron 22 Win7 打包脚本，可选同时构建独立补充包。
- `package.json`：新增 `setup:desktop:win7:gnu:x64`、`doctor:desktop:win7:gnu:x64`、`build:desktop:win7:gnu:x64`、`build:desktop:win7:gnu:x64:fast` 等根命令，便于下次直接重建。
- `patches/win7/tokio-rustls-0.26.4-win7/`：Win7 GNU 试包专用补丁，将 `tokio-rustls` 默认后端切到 `ring`，避免重新拉入 `aws-lc-sys`。
- `Cargo.toml`：对 `target_vendor = "win7"` 单独分流 `reqwest` / `sysinfo` 依赖，降低 Win7 运行时阻塞点。
- `src/core/dpi.rs`：将 DPI API 改为运行时动态加载，避免在 Win7 上因静态导入高版本入口点而提前失败。
- `src/ops/sysinfo_compat.rs`：为 Win7 单独兼容旧版 `sysinfo` API。
- `desktop/electron/scripts/build-win7.ps1`：支持通过 `WUNDER_BRIDGE_BIN` 注入外部 bridge。
- `desktop/electron/scripts/prepare-resources.js`：支持 staging 构建与扩展 runtime 资源注入。

## 5. 已解决的 Win7 阻塞点

### 5.1 `GetSystemTimePreciseAsFileTime`

- 旧的 GNU 包虽然可安装启动，但 bridge 在 Win7 上会因 `KERNEL32.dll` 缺少 `GetSystemTimePreciseAsFileTime` 而中止。
- 根因是当前 Rust `std` 的默认 Windows 基线偏高。
- 解决方式是改为 `x86_64-win7-windows-gnu`，并通过 nightly `-Zbuild-std` 让 `std` 走 Win7 fallback。

### 5.2 `api-ms-win-core-winrt-error-l1-1-0.dll`

- 旧 bridge 修掉时间入口点后，又在 Win7 上触发 `api-ms-win-core-winrt-error-l1-1-0.dll` 缺失。
- 根因是 `reqwest` 默认 `system-proxy` 依赖链会带入 `windows-registry -> windows-result -> winrt-error`。
- 同时，`tokio-rustls` 默认后端还会拉入 `aws-lc-sys`，增加 Win7 验证复杂度。
- 解决方式是：
  - 对 `target_vendor = "win7"` 单独关闭 `reqwest` 默认特性，仅保留 `rustls-tls-webpki-roots`。
  - 通过 `patches/win7/tokio-rustls-0.26.4-win7/` 将 Win7 GNU 试包的 `tokio-rustls` 默认后端切换为 `ring`。

## 6. 为什么当前选这条路

- 相比 Tauri，Electron 不依赖 WebView2，Win7 上前置条件更少。
- 相比 MSVC bridge，GNU bridge 更有机会减少 VC++ 运行库缺失带来的启动失败。
- 相比 `i686` GNU，`x64` GNU 在当前机器和当前工具链上已经实测可走通。
- 相比继续追主线依赖升级，Win7 专用补丁更容易做到对现有发行链零侵入。

## 7. 功能影响

以下影响仅针对 **Win7 GNU 安装包**，不会影响当前主线桌面包、CLI、server 或其他平台构建。

- `reqwest` 不再读取 Windows 系统代理设置；如果部署环境依赖系统代理，需要改为显式环境变量或后续补应用层代理配置。
- `reqwest` 的 TLS 根证书改为 `rustls + webpki-roots`，常见公网 HTTPS/WSS 基本不受影响，但企业内网私有 CA、系统证书仓或代理注入证书场景需要额外验证。
- `tokio-rustls` 在 Win7 GNU 包内改为 `ring` 后端，常规聊天、工具调用、WebSocket / XMPP over TLS 行为应保持一致，但底层密码库实现已与主线不同。
- 监控与系统信息采样走 `sysinfo 0.29.11` 兼容路径，常规 CPU / 内存 / 磁盘采样不应受明显影响，但极端情况下刷新时序与精度可能和主线略有差异。
- Win7 试构建仍默认关闭自动更新，这一点与此前试包策略保持一致。

## 8. 重建建议

### 首次初始化

- 先执行 `npm run setup:desktop:win7:gnu:x64`，一次性准备 nightly、`rust-src`、隔离 Cargo 缓存与 Win7 patch 配置。
- 如只想检查环境，不做预热，可执行 `npm run doctor:desktop:win7:gnu:x64`。

### 日常快速重建

- 代码更新后，优先执行 `npm run build:desktop:win7:gnu:x64:fast`。
- 该入口会复用 `temp_dir/win7-gnu-lab/` 下已经准备好的 nightly、Cargo 缓存、MinGW 路径与 patch 配置，减少重复准备时间。
- 如怀疑依赖缓存脏了或补丁配置需要刷新，再回退到完整入口 `npm run build:desktop:win7:gnu:x64`。

### 配置统一来源

- 下次若要调整 Electron 版本、Rust nightly、目标三元组或 MinGW 路径，只改 `desktop/electron/scripts/win7-gnu-toolchain.json` 一处即可。

## 9. Win7 补充包

- 为 Win7 Electron 试包额外提供一个可直接解压到安装目录根部的 `wunder补充包`，内含 `opt/python` 与 `opt/git`。
- 构建入口：`npm run build:desktop:win7:supplement:x64`
- 基础运行时档位：`npm run build:desktop:win7:supplement:x64:minimal`
- 同时产出安装包与 `common` 补充包入口：`npm run build:desktop:win7:gnu:x64`
- 显式指定 `common` 补充包入口：`npm run build:desktop:win7:gnu:x64:release:common`
- 显式指定 `minimal` 补充包入口：`npm run build:desktop:win7:gnu:x64:release:minimal`
- 构建脚本：`packaging/windows/scripts/build_win7_desktop_supplement.ps1`
- 配置清单：`packaging/windows/scripts/win7-supplement-manifest.json`
- 说明文档：`packaging/windows/README.md`
- 默认产物：`temp_dir/win7-gnu-lab/win7-supplement/dist/wunder补充包-win7-x64-common.zip`
- `minimal` 档位产物：`temp_dir/win7-gnu-lab/win7-supplement/dist/wunder补充包-win7-x64.zip`

当前补充包版本固定为：

- Python：`3.8.10 embeddable`
- Git：`MinGit 2.46.2`

其中 `common` 档位会额外引入一组精简的 Win7 友好 Python 常用依赖，并保留绘图能力：

- `pip / setuptools / wheel`
- `requests / certifi / urllib3`
- `numpy / pandas / openpyxl / xlrd / xlsxwriter / tabulate`
- `python-docx / python-pptx / pypdf2 / lxml`
- `ffmpeg-python / imageio / imageio-ffmpeg / opencv-python-headless`
- `matplotlib / seaborn / plotly / pyecharts / Pillow`
- `folium / pyproj / shapely / netcdf4 / cftime / h5py / arm-pyart / metpy`
- `sqlalchemy / pymysql / aiosqlite`

默认 Python 包索引为清华 Tuna：`https://pypi.tuna.tsinghua.edu.cn/simple`。如需切回官方源，可在脚本层传 `-PythonPackageIndexUrl https://pypi.org/simple`。

另外会参考 ARM sidecar 的做法，在打包阶段将仓库 `fonts/` 中的常用中英文字体注入到 matplotlib 字体目录，提升 Win7 离线绘图与图表导出时的中文渲染稳定性。

这样做的目的，是让用户将补充包直接解压进桌面安装目录后，Electron / bridge 就能自动识别并优先使用内置 Python / Git，而不依赖系统全局安装。

当前 Win7 GNU 构建链已明确改为“安装包不内置 Python/Git，补充包单独分发”的模式。安装完成后，如果需要 Python/Git，只需把对应 `wunder补充包` 解压到安装目录根部。

## 10. 暂不固化的路径

### `i686` GNU

- 当前最小样例就会在 `GetHostNameW@8` 链接阶段失败。
- 这更像是本机 32 位 MinGW 工具链问题，不适合直接纳入默认构建链。

### Tauri GNU

- 当前会卡在资源编译生成的 `resource.lib` 与 GNU 链接器格式不兼容。
- 需要额外调整 `tauri-winres` / `embed-resource` 的资源链路后，才有继续推进价值。

## 11. 下一步建议

- 在真实 Win7 x64 环境继续验证 LLM 请求、知识库下载、插件联网、XMPP / WebSocket 等所有 `reqwest` 相关链路。
- 若目标环境存在企业代理或私有根证书，需要尽快补一套显式代理与自定义 CA 导入方案。
- 若要继续压低运行库依赖，可继续验证 `-StaticRuntime` 版本是否值得作为默认变体。
- 若要继续推进 32 位 GNU，需要先更换一套更稳的 `i686` MinGW / WinLibs / llvm-mingw 工具链。
- 若要继续推进 Tauri GNU，需要单独处理资源编译与链接兼容。
