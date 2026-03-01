# Ubuntu 20.04 编译指引（x86 / arm）

本目录用于在 `glibc 2.31`（Debian bullseye 基线）环境中编译 `wunder-server`、`wunder-cli`、`wunder-desktop`，目标是让产物在 Ubuntu 20.04 上可运行。

> 重要：Ubuntu 20.04 目标的 Desktop 打包统一使用 `wunder-desktop-electron`（Electron AppImage），不要使用 `cargo tauri build` 打包。
> 在 bullseye 基线里，Tauri 依赖链会引入 `glib-2.0 >= 2.70`，与 Ubuntu 20.04 基线不兼容。

- 编译容器编排文件：`docker-extra/docker-compose-ubuntu20.yml`
- x86 Dockerfile：`docker-extra/Dockerfile.ubuntu20-x86`
- arm Dockerfile：`docker-extra/Dockerfile.ubuntu20-arm`

## 1. 前置条件

- 已安装 Docker（推荐 Docker Desktop + Compose v2）
- 在仓库根目录执行命令（`wunder/`）

## 2. 启动编译容器（直接使用现有镜像）

### x86（amd64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml --profile x86 up -d --no-build
```

### arm（arm64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml --profile arm up -d --no-build
```

## 3. 进入容器执行编译与 Desktop 打包（Electron）

### 目录优先级（arm / Ubuntu 20.04）

arm 目标默认优先使用以下缓存与产物目录（推荐）：

- `CARGO_HOME=/app/.cargo/arm64-20`
- `CARGO_TARGET_DIR=/app/target/arm64-20`
- 嵌入式 Python 目录：`/app/target/arm64-20/.build/python`

### 一键打包（推荐）

在仓库根目录执行：

```bash
bash docker-extra/scripts/build_arm64_desktop_with_python.sh
```

该脚本会自动：

- 直接复用 `wunder-arm-20:latest`（`--no-build`）
- 使用 `arm64-20` 目录作为 Cargo 与 target 优先路径
- 生成 Electron arm64 AppImage
- 基于 `target/arm64-20/.build/python` 产出附带 Python 的 AppImage
- 在 AppImage 内补齐 `python -> python3`、`pip -> pip3`，并将 `opt/python/bin` 置于 `PATH` 最前，确保 `执行命令` 工具里的 `python` 默认命中内置解释器
- 内置 Python 依赖默认包含一组“优先、轻量、高价值”库（`orjson`、`tabulate`、`rapidfuzz`、`Unidecode`、`docxtpl`），用于文本匹配、结构化输出、JSON 处理与文档模板生成

> 提示：第 5 步 AppImage 重打包在 qemu 下可能持续 10~30 分钟，看起来像“卡住”但通常仍在运行。
> 可用以下命令观察进度（文件大小持续增长即正常）：
> ```bash
> docker compose -f docker-extra/docker-compose-ubuntu20.yml exec -T wunder-build-arm \
>   bash -lc 'pgrep -af appimagetool || true; ls -lh /app/target/arm64-20/dist/wunder-desktop-arm64-python.AppImage'
> ```

### CI 专用（不附带 Python）

GitHub Actions 的 Linux Nightly 使用以下脚本在 Ubuntu20 基线容器中打包 Electron AppImage（x64/arm64）：

```bash
bash docker-extra/scripts/ci_build_linux_electron.sh
```

### x86（amd64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml exec wunder-build-x86 bash
cargo build --release --bin wunder-server --bin wunder-cli

# build Electron bridge + AppImage（requires frontend/dist）
cargo build --release --bin wunder-desktop-bridge
cd wunder-desktop-electron
npm install
npm run build:linux:x64
```

### arm（arm64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml exec wunder-build-arm bash
export PATH=/usr/local/cargo/bin:$PATH
export CARGO_HOME=/app/.cargo/arm64-20
export CARGO_TARGET_DIR=/app/target/arm64-20
cargo build --release --bin wunder-server --bin wunder-cli

# build Electron bridge + AppImage（requires frontend/dist）
cargo build --release --bin wunder-desktop-bridge
cd wunder-desktop-electron
npm install
WUNDER_BRIDGE_BIN=/app/target/arm64-20/release/wunder-desktop-bridge \
  npm run build:linux:arm64 -- --config.directories.output=/app/target/arm64-20/dist

# package Electron AppImage with embedded Python (uses /app/target/arm64-20/.build/python)
cp "$(ls -1t /app/target/arm64-20/dist/*.AppImage | grep -v python | head -n 1)" \
  /app/target/arm64-20/dist/wunder-desktop-arm64.AppImage
ARCH=arm64 \
APPIMAGE_PATH=/app/target/arm64-20/dist/wunder-desktop-arm64.AppImage \
BUILD_ROOT=/app/target/arm64-20/.build/python \
APPIMAGE_WORK=/app/target/arm64-20/.build/python/appimage \
OUTPUT_DIR=/app/target/arm64-20/dist \
  bash docker-extra/scripts/package_appimage_with_python.sh
```

> NOTE: Linux containers can only produce Linux bundles (AppImage, etc.); Windows MSI must be built on Windows.

## 4. 产物位置

- x86：`target/x86/release/`
- arm（优先）：`target/arm64-20/release/` + `target/arm64-20/dist/`

主要产物：

- `wunder-server`
- `wunder-cli`
- `wunder-desktop-bridge`
- Electron AppImage: `target/arm64-20/dist/wunder-desktop-arm64.AppImage`
- Electron AppImage（附带 Python）: `target/arm64-20/dist/wunder-desktop-arm64-python.AppImage`

## 5. 缓存与目录复用说明

compose 已对齐项目主编排的缓存习惯：

- 构建缓存保持写入仓库目录（bind mount），便于本地清理与管理：
  - x86：`/app/.cargo/x86`、`/app/target/x86`
  - arm（优先）：`/app/.cargo/arm64-20`、`/app/target/arm64-20`
- 会挂载一个命名卷 `wunder_workspaces` 到 `/workspaces`（仅工作区/临时目录）；其余运行态配置仍使用仓库本地 `./data`。

这样能显著减少重复下载依赖和重复编译。

## 6. 常用维护命令

暂停容器（不移除）：

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml --profile x86 --profile arm stop
```

停止并移除容器（不删 volume 缓存）：

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml --profile x86 --profile arm down
```

清理容器 + 卷（会清空 `wunder_workspaces`）：

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml --profile x86 --profile arm down -v
```
