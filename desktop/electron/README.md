# Wunder Desktop Electron

本目录是 Wunder Desktop 的 Electron 外壳，用来打包桌面版应用。核心逻辑是：
1) Electron 启动 Rust 桥接程序（`wunder-desktop-bridge`）。
2) 桥接程序在本地端口启动 Web 服务，并加载已构建的前端资源。
3) Electron 窗口加载该本地地址作为 UI。

## 构建与打包

### 前置条件
- Node.js（建议 22.12+）
- Rust（用于构建 `wunder-desktop-bridge`）
- 已构建的前端产物（`frontend/dist`）

### 步骤一：构建前端
在仓库根目录执行：
```bash
npm install --workspaces --include-workspace-root=false
npm run build --workspace wunder-frontend
```

### 步骤二：构建桥接程序
在仓库根目录执行：
```bash
cargo build --release --bin wunder-desktop-bridge
```

### 步骤三：打包 Electron
```bash
cd desktop/electron

# Linux
npm run build:linux          # 同时输出 x64 + arm64
npm run build:linux:x64
npm run build:linux:arm64

# 其他平台（按需）
npm run build
```

默认产物输出在 `target/desktop-electron/dist/`。
如果需要自定义输出目录，可以使用：
```bash
npm run build:linux:arm64 -- --config.directories.output=../../target/arm64-20
```

JavaScript 依赖统一安装在仓库根目录 `node_modules/`，不要再维护 `frontend/node_modules/` 或 `desktop/electron/node_modules/`。
桌面相关构建产物统一落到仓库根 `target/`；`desktop/electron/` 与 `desktop/tauri/` 目录只保留源码、资源和配置。

### Ubuntu 20.04（arm64）推荐目录

Ubuntu 20.04 目标建议优先使用：

- Cargo 缓存：`.cargo/arm64-20`
- 构建产物：`target/arm64-20`
- 嵌入式 Python：`target/arm64-20/.build/python`

如果希望一键完成 bridge 编译 + Electron 打包 + 附带 Python + Git + ripgrep 重打包，推荐直接在仓库根目录执行：
```bash
bash packaging/docker/scripts/build_arm64_desktop_with_python.sh
```

如果需要生成附带 Python + Git + ripgrep 的 Electron AppImage，可在仓库根目录执行：
```bash
cp "$(ls -1t target/arm64-20/dist/*.AppImage | grep -v python | head -n 1)" \
  target/arm64-20/dist/wunder-desktop-arm64.AppImage
ARCH=arm64 \
APPIMAGE_PATH=target/arm64-20/dist/wunder-desktop-arm64.AppImage \
BUILD_ROOT=target/arm64-20/.build/python \
APPIMAGE_WORK=target/arm64-20/.build/python/appimage \
OUTPUT_DIR=target/arm64-20/dist \
  bash packaging/docker/scripts/package_appimage_with_python.sh
```

该重打包脚本会自动把 `opt/git/bin`、`opt/rg/bin` 与 `opt/python/bin` 提前到 `PATH`，并在缺失时补齐 `python`/`pip` 软链接到 `python3`/`pip3`。这样 `执行命令` 工具里直接跑 `python`/`git`/`rg` 都会优先走内置运行时。

## 资源打包机制

Electron 打包前会执行 `scripts/prepare-resources.js`，将运行所需资源拷贝到 `desktop/electron/resources`：
- `wunder-desktop-bridge`（桥接程序）
- `frontend/dist`（前端静态资源）
- `prompts/`、`skills/`（按仓库资源目录自动解析）
- `config/i18n.messages.json`、`config/wunder.desktop.preconfig.yaml`（如存在）

桌面端安装包不再整包复制仓库 `config/` 或 `scripts/`。运行时配置会在首次启动时生成到 `userData/WUNDER_TEMPD/config/wunder.yaml`，再按可选的桌面预配置文件做一次播种，避免把网页端/服务端配置、数据目录和字体资源混入桌面安装包。

图标现在采用单一源文件（优先）：`images/eva01-head.svg`（若缺失则回退 `images/eva01-head.ico`）。  
`prepare-resources` 前会自动执行 `scripts/sync-icons.js`，统一生成并同步：
- `desktop/electron/build/icon.png`
- `desktop/electron/build/icon.ico`
- `desktop/electron/assets/icon.ico`
- `desktop/tauri/icons/icon.ico`（供 Tauri 打包复用）

`sync-icons.js` 会自动从 `svg` 生成多尺寸 `ico`，并对小尺寸图标加入透明边距（可通过 `WUNDER_ICON_PADDING_RATIO` 调整，默认 `0.06`），避免 Windows 桌面快捷方式出现细边框观感。

如只想手动刷新图标，可执行：
```bash
npm run prepare:icons
```

如果需要指定桥接程序路径，可设置环境变量：
```bash
WUNDER_BRIDGE_BIN=/path/to/wunder-desktop-bridge
```

## Win7 默认流程

当前仓库中，Win7 方向推荐优先使用 **Electron 22 + ia32 GNU bridge**：

- 快速发布与日常重建优先参考：`docs/方案/wunder-desktop-win7-发布SOP.md`
- 更完整的技术背景与兼容性经验见：`docs/方案/wunder-desktop-win7-gnu实验方案.md`

```powershell
# 首次初始化工具链与缓存
npm run setup:desktop:win7:gnu

# Win7 默认正式出包入口
npm run build:desktop:win7:gnu

# 已初始化后的快速重建入口
npm run build:desktop:win7:gnu:fast
```

- GNU 方案的缓存、staging 与安装包统一隔离在 `temp_dir/win7-gnu-lab/`
- Win7 版本默认优先使用 `build:desktop:win7:gnu`（即 `ia32`）这条链路出包
- 默认会同时产出 `setup.exe + wunder补充包-win7-ia32-common.zip`
- Win7 安装包默认不再内置 Python、Git、ripgrep；若需要这三项运行时，请使用同目录产出的 `wunder补充包` zip 手工解压到安装目录
- Win7 `common` 补充包默认通过清华 Tuna 简单索引安装 `packaging/python/requirements-win7-common.txt` 中的依赖；如需切回官方源，可在脚本层传 `-SupplementPythonPackageIndexUrl https://pypi.org/simple`
- 最终安装包路径：`temp_dir/win7-gnu-lab/electron-win7-ia32/dist/wunder-desktop-win7-0.2.0-ia32-setup.exe`
- `common` 补充包路径：`temp_dir/win7-gnu-lab/electron-win7-ia32/dist/wunder补充包-win7-ia32-common.zip`
- 如需显式构建 `x64`，可执行 `npm run build:desktop:win7:gnu:x64`

### Win7 补充包（Python + Git + ripgrep）

Win7 默认发布时，安装包与补充包分开交付。补充包用于按需补齐 Python、Git 与 ripgrep。

如需单独构建补充包，可在仓库根目录执行：

```powershell
npm run build:desktop:win7:supplement
```

- `common` 档位产物：`temp_dir/win7-gnu-lab/win7-supplement/dist/wunder补充包-win7-ia32-common.zip`
- 若只想构建基础运行时，可显式执行 `npm run build:desktop:win7:supplement:minimal`
- `common` 档位会额外内置 `pip / setuptools / wheel`，并按 `packaging/python/requirements-win7-common.txt` 预装办公文档、数据库、轻量视频、地图/雷达、绘图等 Win7 友好依赖
- 默认 Python 包索引为 `https://pypi.tuna.tsinghua.edu.cn/simple`；如需切回官方源，可直接运行脚本并传 `-PythonPackageIndexUrl https://pypi.org/simple`
- 将补充包解压到 Win7 安装目录根部后，目录下应出现 `opt/python`、`opt/git` 与 `opt/rg`
- 详细说明见 `packaging/windows/README.md`
## 运行行为说明

Electron 启动时会：
1) 从 `resources/` 或仓库内寻找 `wunder-desktop-bridge`；
2) 为桥接程序分配空闲端口；
3) 将数据目录设置为 `userData/WUNDER_TEMPD`（临时目录）与 `userData/WUNDER_WORK`（工作区）；
4) 访问 `http://127.0.0.1:<port>/` 加载 UI。

### 可用环境变量
- `WUNDER_BRIDGE_PATH`：覆盖桥接程序路径（运行时）
- `WUNDER_FRONTEND_ROOT`：覆盖前端资源目录（运行时）
- `WUNDER_BRIDGE_BIN`：打包时指定桥接程序路径（prepare-resources）
- `WUNDER_DISABLE_GPU=1`：禁用硬件加速（用于排查 GPU/驱动问题）
- `WUNDER_SUPPRESS_GPU_WARNINGS=0`：关闭默认 GPU 警告抑制（默认启用抑制）
- `WUNDER_LOADING_SHELL_DELAY_MS=0`：配置启动壳页延迟（发布版默认 1200ms，开发模式默认 220ms）
- `WUNDER_BRIDGE_LOG_VERBOSE=0`：关闭 bridge 全量 stdout（默认开启）
- `WUNDER_STARTUP_TIMING=0`：关闭默认启用的启动时序日志
- `WUNDER_SIDECAR_RUNTIME=1`：标记 sidecar 运行态（通常由 sidecar AppRun 自动注入）
- `WUNDER_SIDECAR_FORCE_DISABLE_GPU=0`：关闭 sidecar 默认禁用 GPU 的策略

### UI 行为
- 默认移除菜单栏（避免出现 View / Edit 等菜单）。
- 等待 `ready-to-show` 再显示窗口，减少首帧卡顿。
- 关闭拼写检查与后台节流，提升前台响应。
- Linux AppImage 首次运行会自动写入：
  - `~/.local/share/applications/wunder-desktop.desktop`（开始菜单）
  - `~/Desktop/Wunder Desktop.desktop`（桌面快捷方式，目录存在时）

## 常见问题

### 1) 运行时提示 “todo need put discard in shader”
这是 Chromium/ANGLE/Mesa 的 GPU 日志噪声。
现在默认会抑制常见 GPU 噪声（`MESA_LOG_LEVEL=error`、`MESA_DEBUG=silent`、`LIBGL_DEBUG=quiet`、禁用 Vulkan）。
sidecar 模式默认会禁用 GPU（可用 `WUNDER_SIDECAR_FORCE_DISABLE_GPU=0` + `WUNDER_DISABLE_GPU=0` 还原）。
若仍频繁出现，可尝试设置：
```bash
WUNDER_DISABLE_GPU=1
```

### 2) 找不到 bridge 或 frontend 资源
请先执行前端构建与桥接程序构建，再打包；或使用上面的环境变量指定路径。

## CI Nightly（自动构建与发布）

仓库包含 GitHub Actions 工作流：`.github/workflows/desktop-nightly.yml`。

- 触发方式：`push`（所有分支）或手动 `workflow_dispatch`
- 产物平台：
  - Windows x64
  - macOS（x64 + arm64）
  - Linux x64 / arm64（AppImage，Ubuntu 20 兼容基线，不附带 Python/Git）
- 发布方式：自动更新 `nightly` 标签与 Nightly Release，始终保留最新提交对应产物
- 产物命名示例：`Wunder-Desktop-linux-arm64-YYYYMMDD.AppImage`
