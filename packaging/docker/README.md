# Ubuntu 20.04 构建与 AppImage 打包说明

本目录用于在 `glibc 2.31` 基线环境中构建 Linux 产物，目标是让 `wunder-server`、`wunder-cli`、`wunder-desktop-electron` 在 Ubuntu 20.04 及相近老环境上可运行。

Desktop 的 Linux 打包统一走 `wunder-desktop-electron`，不要用 `cargo tauri build` 产出 Ubuntu 20.04 目标包。

JavaScript 依赖统一走仓库根 `npm workspace`，共享一套根 `node_modules/`。

## 1. 规则先看

- Linux Desktop 统一打包为 Electron AppImage。
- sidecar AppImage 默认不内置 Python，也不内置 Git；运行时依赖同目录的 `wunder补充包`。
- 打包脚本默认不重建 Python。只有显式设置 `ALLOW_PYTHON_REBUILD=1` 或 `FORCE_PYTHON_SYNC=1` 时才会重建。
- AppImage 压缩格式默认使用 `gzip`，优先兼容老系统。
- ARM 与 x86 构建镜像现在都内置了 `squashfs-tools`；若你预先放好了对应架构的 `appimagetool-*.AppImage`，镜像也会把它内置进去，便于离线重打包。

## 2. 关键文件

- 编排文件：`packaging/docker/docker-compose-ubuntu20.yml`
- x86 镜像：`packaging/docker/Dockerfile.ubuntu20-x86`
- ARM 镜像：`packaging/docker/Dockerfile.ubuntu20-arm`
- ARM 一键脚本：`packaging/docker/scripts/build_arm64_desktop_with_python.sh`
- sidecar 打包脚本：`packaging/docker/scripts/package_sidecar_python.sh`
- AppImage 重打包脚本：`packaging/docker/scripts/package_appimage_with_python.sh`

## 3. 构建镜像

### x86

离线重打包推荐预先放好：

- `target/x86-20/.build/python/tools/appimagetool-x86_64.AppImage`

构建命令：

```bash
docker build -t wunder-x86-20:latest -t wunder-x86-20 \
  --platform linux/amd64 \
  -f packaging/docker/Dockerfile.ubuntu20-x86 .
```

### ARM

离线重打包推荐预先放好：

- `target/arm64-20/.build/python/tools/appimagetool-aarch64.AppImage`

构建命令：

```bash
docker build -t wunder-arm-20:latest -t wunder-arm-20 \
  --platform linux/arm64 \
  -f packaging/docker/Dockerfile.ubuntu20-arm .
```

## 4. 启动构建容器

### x86

```bash
docker compose -f packaging/docker/docker-compose-ubuntu20.yml --profile x86 up -d --no-build
```

### ARM

```bash
docker compose -f packaging/docker/docker-compose-ubuntu20.yml --profile arm up -d --no-build
```

## 5. x86 构建流程

`x86` 主要用于 Linux amd64 基础包构建。推荐显式使用 `x86-20` 目录，避免和旧缓存混用。

```bash
docker compose -f packaging/docker/docker-compose-ubuntu20.yml exec -T wunder-build-x86 bash -lc '
  set -euo pipefail
  export CARGO_HOME=/app/.cargo/x86-20
  export CARGO_TARGET_DIR=/app/target/x86-20
  cargo build --release --bin wunder-desktop-bridge
  cd /app
  npm install --prefer-offline --no-audit --no-fund --workspace wunder-desktop-electron
  cd /app/desktop/electron
  WUNDER_BRIDGE_BIN=/app/target/x86-20/release/wunder-desktop-bridge \
    npm run build:linux:x64 -- --config.directories.output=/app/target/x86-20/dist
'
```

默认产物位置：

- `target/x86-20/release/wunder-desktop-bridge`
- `target/x86-20/dist/*.AppImage`

## 6. ARM 一键打包流程

ARM 是当前主路径，推荐直接用一键脚本：

```bash
bash packaging/docker/scripts/build_arm64_desktop_with_python.sh
```

该脚本会执行以下步骤：

1. 启动 `wunder-build-arm` 容器，不重建镜像。
2. 构建 `wunder-desktop-bridge`。
3. 构建 Electron arm64 基础 AppImage。
4. 校验现有 `stage/opt/python` 与 `stage/opt/git`。
5. 产出 `wunder补充包-arm64.tar.gz`。
6. 用 sidecar 模式重打包 `wunder-desktop-arm64-sidecar.AppImage`。

默认目录：

- `CARGO_HOME=/app/.cargo/arm64-20`
- `CARGO_TARGET_DIR=/app/target/arm64-20`
- `BUILD_ROOT=/app/target/arm64-20/.build/python`

## 7. sidecar 预置目录要求

如果你已经有现成的 `wunder补充包`，不要把内容散放到 `.build/python` 根目录。正确目录结构必须是：

- `target/arm64-20/.build/python/stage/opt/python/bin/python3`
- `target/arm64-20/.build/python/stage/opt/git/bin/git`

如果是 x86，同理放到：

- `target/x86-20/.build/python/stage/opt/python/bin/python3`
- `target/x86-20/.build/python/stage/opt/git/bin/git`

Windows 下可用：

```powershell
New-Item -ItemType Directory -Force "target/arm64-20/.build/python/stage" | Out-Null
tar -xzf "D:\备份\wunder补充包-arm64.tar.gz" `
  -C "target/arm64-20/.build/python/stage" `
  --strip-components 1
```

放好后，脚本会直接复用，不会重建 Python。

## 8. 手动重打包 sidecar AppImage

如果你已经有基础 AppImage 和补充包目录，只想重打包 sidecar：

```bash
docker compose -f packaging/docker/docker-compose-ubuntu20.yml exec -T wunder-build-arm \
  bash -lc 'ARCH=arm64 \
  APPIMAGE_PATH=/app/target/arm64-20/dist/wunder-desktop-arm64.AppImage \
  BUILD_ROOT=/app/target/arm64-20/.build/python \
  APPIMAGE_WORK=/app/target/arm64-20/.build/python/appimage \
  OUTPUT_DIR=/app/target/arm64-20/dist \
  PREFER_PREBUILT_PYTHON=1 \
  PREFER_PREBUILT_GIT=1 \
  EMBED_PYTHON=0 \
  EMBED_GIT=0 \
  BUNDLE_PLAYWRIGHT_DEPS=0 \
  PLAYWRIGHT_INSTALL_DEPS=0 \
  APPIMAGE_COMP=gzip \
  bash /app/packaging/docker/scripts/package_appimage_with_python.sh'
```

说明：

- `EMBED_PYTHON=0`：不把 Python 打进 AppImage。
- `EMBED_GIT=0`：不把 Git 打进 AppImage。
- `APPIMAGE_COMP=gzip`：优先兼容老系统。

## 9. 离线打包最低要求

如果你的环境没有外网，至少要提前准备好以下内容。

### x86

- `target/x86-20/.build/python/tools/appimagetool-x86_64.AppImage`
- `target/x86-20/.build/python/stage/opt/python`
- `target/x86-20/.build/python/stage/opt/git`

### ARM

- `target/arm64-20/.build/python/tools/appimagetool-aarch64.AppImage`
- `target/arm64-20/.build/python/stage/opt/python`
- `target/arm64-20/.build/python/stage/opt/git`

如果这些文件齐全，`package_appimage_with_python.sh` 就不会再尝试下载 `appimagetool`，也不会默认重建 Python。

## 10. 运行时日志与调试开关

打包后的 AppImage 在终端里看起来“几乎没日志”，这是当前默认策略，不是终端问题。

默认行为如下：

- 发布版默认开启 `WUNDER_STARTUP_TIMING`，会打印 `[startup]` 时序日志。
- Electron 主进程默认打印 bridge 全量 stdout。
- Linux 默认开启 GPU 噪声抑制，因此 Mesa / EGL / Vulkan 警告会被压掉。
- Rust bridge 在发布版默认按 `info` 输出；如果显式关闭启动时序，再未设置 `RUST_LOG` 时会退回 `warn`。

相关环境变量：

- `WUNDER_STARTUP_TIMING=0`：关闭默认启用的启动时序日志。
- `WUNDER_BRIDGE_LOG_VERBOSE=0`：关闭默认启用的 bridge 全量 stdout。
- `RUST_LOG=info`：提升 Rust 侧日志级别。
- `WUNDER_SUPPRESS_GPU_WARNINGS=0`：关闭 GPU 警告抑制。
- `WUNDER_CHROMIUM_LOG_LEVEL=2`：放宽 Chromium 日志过滤。

示例：

```bash
WUNDER_STARTUP_TIMING=1 \
WUNDER_BRIDGE_LOG_VERBOSE=1 \
RUST_LOG=info \
./wunder-desktop-arm64-sidecar.AppImage
```

如果你还想把 GPU 相关输出也放出来：

```bash
WUNDER_STARTUP_TIMING=1 \
WUNDER_BRIDGE_LOG_VERBOSE=1 \
RUST_LOG=info \
WUNDER_SUPPRESS_GPU_WARNINGS=0 \
WUNDER_CHROMIUM_LOG_LEVEL=2 \
./wunder-desktop-arm64-sidecar.AppImage
```

## 11. 常见问题

### 1. sidecar AppImage 能启动，但业务依赖报错

先检查补充包是否完整，尤其是：

- `opt/python`
- `opt/git`
- Python 扩展模块是否与当前架构一致

如果 `cartopy`、`pyproj._crs`、`shapely.lib`、`h5py._proxy` 这类模块缺失，问题不在 AppImage 本体，而在补充包内容不完整或版本不匹配。

### 2. 打包出的 AppImage 在老机器上报 SquashFS compression 不支持

优先确认使用了：

- `APPIMAGE_COMP=gzip`
- 镜像里安装了 `squashfs-tools`

不要依赖某些旧版内置 `mksquashfs` 的默认压缩格式。

### 3. sidecar AppImage 为什么不带 Git

这是当前默认设计。sidecar 版本统一从 `wunder补充包` 提供 `git`，避免 AppImage 本体重复塞一份工具链。

## 12. 产物位置

### x86

- `target/x86-20/release/wunder-desktop-bridge`
- `target/x86-20/dist/*.AppImage`

### ARM

- `target/arm64-20/release/wunder-desktop-bridge`
- `target/arm64-20/dist/wunder-desktop-arm64.AppImage`
- `target/arm64-20/dist/wunder-desktop-arm64-sidecar.AppImage`
- `target/arm64-20/dist/wunder-desktop-arm64-python.AppImage`
- `target/arm64-20/dist/wunder补充包-arm64.tar.gz`

## 13. 常用维护命令

停止容器但保留：

```bash
docker compose -f packaging/docker/docker-compose-ubuntu20.yml --profile x86 --profile arm stop
```

删除容器但保留卷：

```bash
docker compose -f packaging/docker/docker-compose-ubuntu20.yml --profile x86 --profile arm down
```

删除容器和卷：

```bash
docker compose -f packaging/docker/docker-compose-ubuntu20.yml --profile x86 --profile arm down -v
```

