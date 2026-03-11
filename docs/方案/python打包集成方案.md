# Python 打包集成方案（AppImage + Python Sidecar，Ubuntu 20.04）

## 1. 目标与约束
- 目标：桌面端默认采用 **AppImage + Python Sidecar** 的交付形态，减少 AppImage 体积并便于独立升级 Python 运行时。
- 平台：Ubuntu 20.04（glibc 2.31），需要确保二进制与动态库兼容。
- 依赖：运行时不依赖系统在线安装（无 apt 在线安装、无 pip 在线安装）。
- 交付：默认两个产物（AppImage + sidecar 补充包），可选单文件 AppImage（内置 Python）。

## 2. 方案概览（推荐）
**推荐方案：AppImage + Python Sidecar（默认）**
- 在 Ubuntu 20.04 构建 Python 基座与离线依赖仓库（wheelhouse）。
- Python + Git 以 sidecar 形式独立打包（`wunder补充包`），与 AppImage 同目录分发。
- AppRun 在启动时自动识别同目录 `wunder补充包`，设置 `WUNDER_PYTHON_BIN` / `WUNDER_GIT_BIN` 与运行时环境。
- 可选：Playwright 浏览器单独 sidecar（`wunder-playwright`），避免 AppImage 体积暴涨。

## 3. 方案对比
| 方案 | 优点 | 风险 / 缺点 | 适配度 |
| --- | --- | --- | --- |
| **A. AppImage + Python Sidecar（推荐）** | 体积小、升级灵活、可独立分发浏览器 | 交付为多文件 | ⭐⭐⭐⭐⭐ |
| B. AppImage 内置 Python | 单文件开箱即用 | 体积大、发布成本高 | ⭐⭐⭐ |
| C. conda-pack / Miniconda | 包管理方便 | 体积大、许可与依赖复杂 | ⭐⭐ |
| D. PyInstaller / Nuitka | 交付简单 | 对大量依赖不友好、可维护性差 | ⭐⭐ |

## 4. 目录结构与运行约定
### 4.1 分发目录（推荐）
```
release/
  wunder-desktop-arm64-sidecar.AppImage
  wunder补充包/                   # 解压后的补充包（含 python/git）
  wunder-playwright/             # 可选：浏览器 sidecar
```
- `wunder补充包` 目录必须与 AppImage **同级**，AppRun 会自动识别。
- `wunder-playwright` 目录同级时，会自动设置 `PLAYWRIGHT_BROWSERS_PATH`。

### 4.2 AppDir 内部（AppImage）
```
AppDir/
  AppRun
  usr/bin/wunder-desktop
  opt/git/...
  # 默认不再内置 Python
```

## 5. 构建流程（默认：Sidecar）
### 5.1 构建 Python 基座与依赖
使用 Ubuntu 20.04 容器执行：
```
BUILD_ROOT=/app/target/arm64-20/.build/python \
  bash /app/packaging/docker/scripts/build_embedded_python.sh
```
- Python 会安装到 `${BUILD_ROOT}/stage/opt/python`。
- 依赖清单默认使用 `packaging/python/requirements-full.txt`。
- 脚本会在安装后自动校验 `matplotlib/cartopy/pyproj/shapely/netCDF4/cftime/h5py` 等核心库；若缺失会尝试补装，并输出 `${BUILD_ROOT}/reports/stage-pip-freeze.txt`、`${BUILD_ROOT}/reports/stage-pip-list.json`、`${BUILD_ROOT}/reports/stage-import-validation.json` 供排查。

### 5.2 打包补充包 Sidecar
```
BUILD_ROOT=/app/target/arm64-20/.build/python \
  bash /app/packaging/docker/scripts/package_sidecar_python.sh
```
- 产物：`wunder补充包-<arch>.tar.*`
- 解压后目录名为 `wunder补充包`（必须保持该名称，内部包含 `opt/python` 与 `opt/git`）。
- 默认会从 `fonts/` 中拷贝常用中英文字体到 matplotlib 字体目录，并生成 `opt/python/etc/matplotlibrc`，用于自动解决中文缺字问题。
- 内置气象/地理绘图库（cinrad/PyCINRAD、arm_pyart、netCDF4、cartopy 等），并预下载 cartopy 的地图数据到 `opt/python/share/cartopy` 以支持离线绘图。
- 内置常用 Jupyter/OpenAI/爬虫/数据库连接库，适合本地分析与数据采集。

### 5.3 重打包 Sidecar AppImage（不内置 Python）
```
ARCH=arm64 \
APPIMAGE_PATH=/app/target/arm64-20/dist/wunder-desktop-arm64.AppImage \
BUILD_ROOT=/app/target/arm64-20/.build/python \
APPIMAGE_WORK=/app/target/arm64-20/.build/python/appimage \
OUTPUT_DIR=/app/target/arm64-20/dist \
PREFER_PREBUILT_PYTHON=1 \
PREFER_PREBUILT_GIT=1 \
EMBED_PYTHON=0 \
BUNDLE_PLAYWRIGHT_DEPS=0 \
PLAYWRIGHT_INSTALL_DEPS=0 \
  bash /app/packaging/docker/scripts/package_appimage_with_python.sh
```
- 输出：`wunder-desktop-*-sidecar.AppImage`
- AppRun 会自动识别同目录 `wunder补充包` 并注入 `WUNDER_PYTHON_BIN` / `WUNDER_GIT_BIN`。
- 默认优先使用 `gzip` 压缩（兼容旧版 FUSE/squashfs 环境）；可显式设置 `APPIMAGE_COMP=zstd` 获取更快冷启动，或 `APPIMAGE_COMP=xz` 追求更小体积。

## 6. 可选：内置 Python（单文件 AppImage）
若需要单文件交付，使用内置模式：
```
EMBED_PYTHON=1 \
  bash /app/packaging/docker/scripts/package_appimage_with_python.sh
```
- 输出：`wunder-desktop-*-python.AppImage`
- 体积更大，但使用体验最省心。

## 7. Playwright 与浏览器（可选）
### 7.1 安装 Playwright + Chromium
```
INCLUDE_PLAYWRIGHT=1 BUILD_ROOT=/app/target/arm64-20/.build/python \
  bash /app/packaging/docker/scripts/build_embedded_python.sh
```
- 浏览器默认安装在 `${PYTHON_ROOT}/playwright`。

### 7.2 浏览器独立 Sidecar（推荐）
- 将 `${PYTHON_ROOT}/playwright` 打包为 `wunder-playwright` 目录，与 AppImage 同级分发。
- 或将 `playwright` 目录放入 `wunder补充包/playwright`，AppRun 也可自动识别。
- AppRun 会自动设置 `PLAYWRIGHT_BROWSERS_PATH`。

> 说明：若目标系统缺少 `libnss3/libnspr4` 等运行库，可在 AppImage 重打包阶段使用
> `BUNDLE_PLAYWRIGHT_DEPS=1` 自动收集依赖到 `usr/lib/wunder-playwright`。

## 8. 关键环境变量
- `WUNDER_PYTHON_BIN`：指向内置或 sidecar 的 `python3`。
- `PYTHONHOME` / `PYTHONPATH`：由 AppRun 自动注入。
- `PLAYWRIGHT_BROWSERS_PATH`：若存在 `wunder-playwright` 或 `${PYTHON_ROOT}/playwright` 则自动注入。
- `CARTOPY_DATA_DIR`：若存在 `${PYTHON_ROOT}/share/cartopy` 则自动注入（离线地图数据）。

## 9. 验收清单
- Ubuntu 20.04 无系统 Python 环境可启动 AppImage。
- `wunder补充包` 在同目录时，工具调用能命中 `WUNDER_PYTHON_BIN` / `WUNDER_GIT_BIN`。
- 断网环境下可用（pip 不访问网络）。
- `ldd` 检查无 “not found”。
- sidecar / embedded 打包前会强校验核心地理绘图库；若缺少 `cartopy`、`pyproj`、`shapely`、`netCDF4`、`h5py` 等，不允许继续出包。

## 10. 交付清单
- `wunder-desktop-*-sidecar.AppImage`
- `wunder补充包-<arch>.tar.*`（解压为 `wunder补充包/`）
- （可选）`wunder-playwright-<arch>.tar.*` 或 `wunder-playwright/`
- （可选）`wunder-desktop-*-python.AppImage`（内置 Python）

## 11. 里程碑 / 节点
1) 依赖清单冻结（`requirements-full.txt`）
2) Python 基座构建完成（Ubuntu 20.04 编译或可复用基座）
3) Sidecar Python 产出
4) AppImage 重打包完成（sidecar 模式）
5) 联调与验收（无 Python、断网、依赖完整性检查）
6) 发布与回滚策略（版本号、校验和、可回滚旧包）

---

**结论**：默认使用 AppImage + Python Sidecar 方案可显著降低体积，并保持运行时稳定性与可维护性；如需单文件交付，可切换内置 Python 模式。

