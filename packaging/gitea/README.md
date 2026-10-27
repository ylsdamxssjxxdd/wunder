# ARM64 离线 Gitea 与双架构 CI

本包面向已有 Docker Engine 与 Docker Compose v2 的 ARM64 Linux 服务器。蛹是本地 AI 工作台，默认 CI 构建蛹 Linux AppImage 和舵机独立可执行文件，均提供 arm64、amd64 两种架构，保持 Ubuntu 18.04/glibc 2.27 基线。AMD64 使用 ARM64 原生 Rust 交叉编译，不依赖整机 x86 仿真。

## 启动

1. 把整个目录复制到服务器的可写目录，保留 `compose.yaml`、`images/` 等相对位置。
2. 执行 `bash load-images.sh`，校验并加载三个 ARM64 镜像，然后校验、解压独立的 `rust-builder/offline-sdk.tar.gz`，自动记录 SDK 的宿主机绝对路径。不需要手动修改 Runner 挂载路径。
3. 加载脚本会在不存在时复制 `.env.example` 为 `.env`。编辑 `.env`，将 `GITEA_PUBLIC_URL` 改为浏览器可访问的服务器地址，例如 `http://<服务器地址>:3000/`；`GITEA_SSH_HOST` 只填主机名或地址，不带协议。HTTP/SSH 端口默认 3000/2222。
4. 执行 `docker compose up -d`。镜像策略为 `never`，不会访问外部镜像仓库。初始化、管理员创建和 Runner 注册自动完成。
5. 在服务器本地执行 `sudo cat data/bootstrap/admin-password` 获取随机初始密码，使用 `.env` 中的管理员用户名登录。首次登录必须修改密码，不要把密码文件发送给他人。

查看状态：

```bash
docker compose ps -a
docker compose logs --tail=100 gitea bootstrap runner
```

默认关闭自助注册与外部头像，私有仓库需登录。HTTP 不加密，仅用于可信隔离内网；跨不可信网络请自行配置 HTTPS。管理员密码和 Runner 注册信息由部署时随机生成，不预置在镜像或本包中。

## SDK 只读挂载

离线包不放入任何镜像。目录结构为：

```text
compose.yaml
images/                         Gitea、Runner、轻量构建镜像
rust-builder/offline-sdk.tar.gz  独立 Rust-builder 离线包
rust-builder/offline/            首次部署时解压，后续直接使用
data/                           启动后生成的代码、账户、运行数据
```

构建镜像仅包含 Ubuntu 18.04 系统构建依赖和 CI 辅助脚本。`offline-gitea-sdk` 是绑定到宿主目录的命名卷：Runner 与它创建的每个任务容器都将此卷只读挂到 `/opt/offline-sdk`，不只是给 Runner 自身挂载。编译输出和 Cargo 配置写到独立的 `offline-gitea-build-cache`，不会污染 SDK。

已有离线目录可复用，先执行 `WUNDER_SDK_PATH=/srv/build-sdk/offline bash prepare-sdk.sh`，再设置服务地址并 `docker compose up -d`。这里的路径必须是 Docker daemon 所在宿主机的绝对路径，不能写成只存在于 Runner 容器内的路径。

本包保留 ARM 离线包中的稳定版与 nightly Rust、Cargo vendor、双架构 Linux sysroot、AppImage runtime、MinGW 和 WASM 工具，不依赖原准备机的 Cargo home、登录信息或编译缓存。默认工作流仍仅构建 Linux arm64/amd64；保留 Win7/WASM 工具不等于已配置和验证相应发布任务。

更新 SDK 无需重建镜像：停止 Runner 和正在执行的任务后，更换同一路径下的离线目录，再启动 Runner。若改成另一个绝对路径，先 `docker compose down`，再仅移除 `docker volume rm offline-gitea-sdk`，执行上述 `prepare-sdk.sh` 后重新启动，避免 Docker 复用旧卷的绑定路径。该卷只绑定 SDK，不包含仓库数据库；不要使用 `down -v`。移动整个部署目录时同样需要重新记录路径并重建此绑定卷。

## 推送后自动构建

在 Gitea 创建空仓库，不初始化 README。本项目已提供 `.gitea/workflows/offline-linux.yml`，必须先由仓库所有者把该文件与 `packaging/gitea/ci/` 纳入提交，然后推送：

```bash
git remote add gitea http://<服务器地址>:3000/<用户名>/<仓库名>.git
git push gitea main
git push gitea --tags
```

新仓库默认开启 Actions；已有仓库请在「设置 → 仓库 → 单元」确认启用 Actions。进入 Actions 页面查看 `Offline Linux Release`：每次分支推送、`v*` 标签推送或手工触发串行执行 ARM64、AMD64 构建，实际先后顺序由调度器决定。运行日志保留在 Gitea；产物通过该次运行的短期令牌自动上传至仓库 Releases，无需长期 PAT。

仓库里原来的 `.gitea/workflows/ci.yml` 在线手工流水线已由离线工作流替代，不再要求安装 Rust 或访问 GitHub Actions。普通 `git push` 只上传 Git 历史；若仓库还使用 Git LFS，迁移时需由客户端把相应 LFS 对象另外推送到新远程。当前默认构建不依赖 GLB 模型等 LFS 资源，不自动下载子模块或外部资源。

每次运行创建独立预发布版，标签形如 `ci-<提交短哈希>-<运行编号>`。两个架构完成后包含：

- `wunder-slint-<版本>-linux-arm64.AppImage` 与 `wunder-slint-<版本>-linux-amd64.AppImage`。
- `wunder-cli-<版本>-linux-arm64` 与 `wunder-cli-<版本>-linux-amd64`。
- 每架构 SHA256 清单，以及 amd64 原生 ELF（沿用项目构建脚本）。

某个架构失败时另一架构可继续；Release 已有附件不代表全部成功，应以 Actions 两项构建状态为准。这里不重新生成或捆绑浏览器/Python 等补充包，不包含 Windows/Win7 安装器，也不替代 `.github/workflows/` 的在线发布流水线。

## 离线边界与资源

- Rust crate vendor、Rust ARM64 工具链、amd64 标准库、双架构 Ubuntu 18.04 sysroot、AppImage runtime 由独立 SDK 目录只读挂载，未嵌入构建镜像。流程不使用在线 `actions/checkout`、`upload-artifact`、`rustup`、`apt` 或 `npm install`。
- 构建容器仅加入 Compose 内部网络，只能与本地服务通讯；Gitea 另有入口网络供浏览器和 Git 客户端访问。仅 Runner 持有 Docker socket，构建容器不挂载 socket，也不挂载管理员/Runner 凭证目录。
- 仍然只允许可信仓库执行 CI。Docker socket 等价宿主机高权限；同 Runner 的项目共享可写缓存，不提供不可信多租户隔离。不要给陌生项目启用此 Runner。
- 默认单个任务、两个架构串行、每次 Cargo 4 线程。建议至少 8 核、16 GB 内存、100 GB 可用存储；首次 Rust release 构建耗时明显，之后复用 `offline-gitea-build-cache` 命名卷。按硬件可调整 `runner.yaml` 中的 `WUNDER_BUILD_JOBS`、`CARGO_BUILD_JOBS`，不要盲目提高并发。
- 新增/升级依赖超出 SDK 中的 vendor 时，离线构建会明确失败，需在联网准备机更新离线包并替换挂载目录；仅更新 Rust/vendor/sysroot 不必重建镜像，只有系统构建依赖变化时才需更新镜像。不可能自动获取离线环境中不存在的依赖。
- 本包不包含 Docker Engine、Compose 插件或宿主内核安装包，服务器须事先具备 ARM64 Docker 和 Compose v2。固定的网络与缓存卷名称供 Runner 定位，默认只支持同一 Docker 主机部署一套本包，避免与其他同名部署混用。
- 构建结果、源码、SQLite 数据库随 `data/gitea/` 持久化；Runner 注册存储在 `data/runner/`，启动凭证在 `data/bootstrap/`。备份时先 `docker compose stop` 再备份整个 `data/`，完成后 `docker compose up -d`。不要删除 Runner 注册文件或 bootstrap 凭证。
- 删除旧 Release 附件、按需停机清理缓存卷，可控制长期磁盘增长；不要运行 `docker compose down -v`，除非确实要删除缓存。镜像与卷保留策略不会自动删除代码或发布产物。

## 其他项目与维护

`examples/` 包含工作流和构建脚本模板。本包针对本项目 Rust/Slint 构建，并非能离线构建任意依赖的通用 GitHub 托管执行器。其他项目应按实际依赖制作专用构建镜像。

在联网开发机重建与导出本包：

```powershell
python packaging/gitea/prepare-bundle.py --sdk ../Rust-builder/kylin-arm/offline --output <输出目录>
```

需要准备机现有 `wunder-slint-arm64-ubuntu18:latest` 和完整 ARM64 离线 SDK；脚本通过 Linux tar 单独归档 SDK，保留符号链接与执行权限，不再把 SDK 导入或复制到镜像。导出采用 gzip 流式压缩，不在内存中装载整个镜像。镜像版本与 ID、压缩文件大小和 SHA256 记录在 `images/manifest.json`，SDK 校验和单独保存在 `rust-builder/SHA256SUMS`。仅更新配置/镜像时可增加 `--reuse-sdk-archive` 复用已校验的 SDK 归档；该参数不会重新收集 SDK 修改。

官方参考：Gitea Docker 安装、Gitea Runner Docker 安装、Runner 配置、Gitea Actions 文档。此包固定 Gitea 28.2.0 与 Gitea Runner 5.2.3，不使用漂移的 latest 标签。

已完成的验证和未验证边界见同目录 `VALIDATION.md`。离线 CI 基础链路与小型双架构编译验证通过，不等于本项目完整 Release 已构建成功；完整项目首次构建请以 Actions 实际状态为准。
