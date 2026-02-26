# Ubuntu 20.04 编译指引（x86 / arm）

本目录用于在 `glibc 2.31`（Debian bullseye 基线）环境中编译 `wunder-server`、`wunder-cli`、`wunder-desktop`，目标是让产物在 Ubuntu 20.04 上可运行。

- 编译容器编排文件：`docker-extra/docker-compose-ubuntu20.yml`
- x86 Dockerfile：`docker-extra/Dockerfile.ubuntu20-x86`
- arm Dockerfile：`docker-extra/Dockerfile.ubuntu20-arm`

## 1. 前置条件

- 已安装 Docker（推荐 Docker Desktop + Compose v2）
- 在仓库根目录执行命令（`wunder/`）

## 2. 构建并启动编译容器

### x86（amd64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml --profile x86 up -d --build
```

### arm（arm64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml --profile arm up -d --build
```

## 3. 进入容器执行编译

### x86（amd64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml exec wunder-build-x86 bash
cargo build --release --bin wunder-server --bin wunder-cli
# build desktop binary only
cargo build --release --features desktop --bin wunder-desktop
# build Linux AppImage (requires frontend/dist to exist)
cargo tauri build --features desktop --config wunder-desktop/tauri.conf.json --bundles appimage

# build Linux AppImage with embedded Python (base requirements)
ARCH=x86 bash docker-extra/scripts/package_appimage_with_python.sh

# build Electron AppImage (requires frontend/dist + bridge binary)
cargo build --release --bin wunder-desktop-bridge
cd wunder-desktop-electron
npm install
npm run build:linux:x64
```

### arm（arm64）

```bash
docker compose -f docker-extra/docker-compose-ubuntu20.yml exec wunder-build-arm bash
cargo build --release --bin wunder-server --bin wunder-cli
# build desktop binary only
cargo build --release --features desktop --bin wunder-desktop
# build Linux AppImage (requires frontend/dist to exist)
cargo tauri build --features desktop --config wunder-desktop/tauri.conf.json --bundles appimage

# build Linux AppImage with embedded Python (base requirements)
bash docker-extra/scripts/package_appimage_with_python.sh

# build Electron AppImage (requires frontend/dist + bridge binary)
cargo build --release --bin wunder-desktop-bridge
cd wunder-desktop-electron
npm install
npm run build:linux:arm64
```

> NOTE: Linux containers can only produce Linux bundles (AppImage, etc.); Windows MSI must be built on Windows.

## 4. 产物位置

- x86：`target/x86/release/`
- arm：`target/arm64/release/`

主要产物：

- `wunder-server`
- `wunder-cli`
- `wunder-desktop`
- `wunder-desktop-bridge`
- Electron AppImage: `wunder-desktop-electron/dist/*.AppImage`

## 5. 缓存与目录复用说明

compose 已对齐项目主编排的缓存习惯：

- 构建缓存保持写入仓库目录（bind mount），便于本地清理与管理：
  - x86：`/app/.cargo/x86`、`/app/target/x86`
  - arm：`/app/.cargo/arm64`、`/app/target/arm64`
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
