# 离线 Gitea 与双架构 CI（arm64 / x86_64 剖面）

本包面向已有 Docker Engine 与 Docker Compose v2 的 Linux 服务器，一套脚本按 `--arch` 导出两种剖面：
`arm64`（ARM64 宿主，amd64 目标靠原生交叉）与 `amd64`（x86_64 宿主，arm64 与 Win7 32 位靠 SDK 内交叉工具链）。
两者都不需要 QEMU，都保持 Ubuntu 18.04 / glibc 2.27 基线。蛹（wunder）默认构建 Linux 桌面归档与舵机（wunder-cli）可执行文件，
x86_64 剖面额外构建 Win7 32 位与补充包。

## 启动

1. 把整个目录复制到服务器的可写目录，保留 `compose.yaml`、`images/`、`rust-builder/` 的相对位置。
2. 执行 `bash load-images.sh`。它先按 `.env` 的 `CI_PLATFORM` 校验宿主架构（x86_64 套件不能在 ARM 机上装，反之亦然），
   再校验并加载三个镜像，然后校验、解压 `rust-builder/offline-sdk.tar.gz` 与 `rust-builder/supplement-prebuilt.tar.gz`，
   自动把 SDK 的宿主机绝对路径记进 `.env`。不需要手工改 Runner 挂载路径。
3. 编辑 `.env`，把 `GITEA_PUBLIC_URL` 改成浏览器可访问的地址（如 `http://<服务器地址>:3000/`）；
   `GITEA_SSH_HOST` 只填主机名或地址。HTTP/SSH 端口默认 3000/2222。
4. `docker compose up -d`。镜像策略为 `never`，不会访问外部镜像仓库；初始化、管理员创建、Runner 注册自动完成。
5. `sudo cat data/bootstrap/admin-password` 取初始密码登录，首次登录必须改密码。

```bash
docker compose ps -a
docker compose logs --tail=100 gitea bootstrap runner
```

默认关闭自助注册与外部头像，私有仓库需登录。HTTP 不加密，仅用于可信内网。

## 剖面、标签与挂载

`.env` 里由导出脚本写入的剖面参数：`CI_PLATFORM`、`SDK_PROFILE`（kylin-arm / kylin-x86）、
`STACK_PREFIX`（卷与网络前缀，arm64 为 `offline-gitea`，x86_64 为 `offline-gitea-x86`，两套可在同一台机器并存）、
`GITEA_IMAGE`、`RUNNER_IMAGE`、`BUILDER_IMAGE`、`RUNNER_LABELS`，以及 `GITEA_DISABLE_REGISTRATION`/
`GITEA_REQUIRE_SIGNIN_VIEW`（套件默认关闭自助注册并要求登录，个人机器可置 false 放开）。

Runner 注册两个标签，都指向同一个任务镜像，只是给两个仓库各留一条路由：

- `wunder-offline` → 任务镜像（Ubuntu 18.04 + Rust/Slint 交叉工具链 + git + `/opt/offline-ci`），构建 Linux 桌面/CLI、Win7 32 位交叉，并复用预制补充包。
- `rcho-offline` → 同一个镜像，供 rcho 的离线流水线使用。rcho 仓库自带 `.gitea/workflows/offline.yml` 与 `packaging/gitea/ci/`，推到同一实例即可构建 Linux amd64/arm64 与 Win7 x86；检出前只能调用镜像里烘焙的 `/opt/offline-ci/checkout.py`，检出后一律改用仓库内脚本。因为 rcho 的构建器会往 SDK 的 `cargo-home-*` 写配置，它的 `prepare-view.sh` 把只读 SDK 组织成 `/ci-cache` 下的可写视图（只读内容用符号链接引用），任何情况下都不向 `/opt/offline-sdk` 写入。

`runner.yaml` 是模板，`@TOKEN@` 由 `prepare-bundle.py` 渲染，套件里拿到的是已渲染值；标签与卷名必须和 `.env`/`compose.yaml` 同源。
`<STACK_PREFIX>-sdk` 卷把离线目录只读挂到 `/opt/offline-sdk`，Runner 与它创建的每个任务容器都带这个挂载；
编译输出与 Cargo 配置写入 `<STACK_PREFIX>-build-cache`，不污染 SDK。

已有离线目录可复用：`WUNDER_SDK_PATH=/srv/build-sdk/offline bash prepare-sdk.sh`。路径必须是 Docker 宿主机绝对路径。
换 SDK 不必重建镜像：停 Runner 与在跑任务，替换同路径目录，再启 Runner；改了绝对路径则 `docker compose down` 后
`docker volume rm <STACK_PREFIX>-sdk`，重跑 `prepare-sdk.sh`。不要 `down -v`。

## 推送后自动构建

仓库需包含 `.gitea/workflows/offline-linux.yml` 与 `packaging/gitea/ci/`，然后推送：

```bash
git remote add gitea http://<服务器地址>:3000/<用户名>/<仓库名>.git
git push gitea main
```

`Offline Linux Release` 在每次分支推送、`v*` 标签推送或手工触发时执行，交付物与 GitHub 的
`nightly` 发布同形——三个桌面程序 + 三个安装包，补充包不进 Release：

- `linux`：矩阵 arm64/amd64。桌面 `wunder-slint-<版本>-linux-<arch>.tar.gz`、便携包 `wunder-<版本>-<x64|arm64>.portable`、
  安装包 `wunder-<版本>-<x64|arm64>.run`；裸 CLI 只作架构与 glibc 门禁的样本，组完包转存到缓存卷，不当附件发布。
  `verify.py` 校验 ELF 架构、AppDir 布局、桌面二进制的 glibc ≤ 2.27，以及两个自解压件的头部标记与 gzip 载荷。
- `win7`（仅 x86_64 worker 上真构建）：`wunder-frontend-slint-<版本>-win7-x86.exe`，`verify.py` 校验 PE i386 头；
  同时把桌面与 CLI 两个 PE 写进 `/ci-cache/<仓库键>/win7-x86/installer-inputs/` 交给下一个任务。
- `win7-installer`（`needs: win7`）：在带 Wine + Inno Setup 的第四个镜像里跑 `ISCC.exe`，用交接 PE 与预置的
  `wunder-supplement-win7-ia32-common.zip` 组出 `wunder-<版本>-win7-x86-setup.exe`。ARM worker 上没有可用的
  i386 Wine，脚本按 `uname -m` 打印 SKIP 并成功退出（所以 arm64 剖面把该标签指向构建镜像本身，不留空标签）。

检出后的步骤一律使用仓库内 `packaging/gitea/ci/` 脚本，只有检出本身用镜像内 `/opt/offline-ci/checkout.py`。
产物由该次运行的短期令牌上传到 Releases，标签形如 `ci-<提交短哈希>-<运行编号>`。
某个任务失败不影响其他任务；Release 有附件不代表全部成功，以 Actions 任务状态为准。

## 离线边界与资源

- Rust crate vendor、双架构工具链与 sysroot、MinGW、AppImage runtime 由 SDK 目录只读挂载。
  补充包（Linux 的 opt/python、opt/git、opt/rg、opt/ffmpeg、opt/browser 与 Win7 的 ia32 同名组件）
  **不在离线端构建**：它要在目标 libc 上编译 git 并铺 Python 运行时，离线机没有这些上游归档与 wheel 索引，
  所以在联网准备机上产好后随套件导出，放在 `supplement-prebuilt/<arch>/`（`amd64`、`arm64`、`win7-ia32` 三份，
  各带 `.sha256`）。离线端只把它们当输入：Linux 的 `.run` 用对应架构的 tar.gz，Win7 安装包用那份 zip；
  用前先复核 SHA256，不匹配就失败，补充包本身不作为 Release 附件。
  准备方法与目录说明见 `cache/`（准备机侧，不进套件）。
- Win7 安装包的镜像是套件里唯一的非 18.04 基线镜像（Ubuntu 22.04 + Wine + Inno Setup 6.7.3）：它不编译 Rust，
  只跑 `ISCC.exe`，所以可以用新发行版；bionic 自带的 Wine 3.0 跑不动 Inno Setup 6.7。Inno 的下载版本与
  SHA256 和 `build_win7_installer.ps1` 固定一致，镜像在准备机上构建、以离线归档交付。
- 流程不使用 `actions/checkout`、`upload-artifact`、`rustup`、`apt`、`npm install` 或任何公网源。
  任务容器只加入 `internal` 网络，无外网出口；仅 Runner 持有 Docker socket。
- 新增/升级依赖超出 vendor 或 wheel 索引时离线构建会明确失败，需在联网准备机刷新
  在准备机上重产补充包（`cache/offline_build.sh <arch>`）并用 `--supplement-prebuilt` 重新导出套件。
- 补充包依赖（requirements-linux-common.txt）或工具链变化后同样要重产重导。x86_64 剖面默认 3 个并行任务、每任务 6 线程（建议 ≥16 核、32 GB 内存、200 GB 可用存储）；
  arm64 剖面默认 1 并行、4 线程。改 `prepare-bundle.py` 的 `runner_capacity`/`build_jobs` 后重新导出。
- 数据在 `data/gitea/`（仓库、账号、SQLite、Release 附件），Runner 注册在 `data/runner/`，
  启动凭据在 `data/bootstrap/`。备份先 `docker compose stop` 再拷 `data/`。旧 Release 需定期清理，
  仓库工作流可用 `KEEP_CI_RELEASES` 之类的留存策略，本包不自动删除发布。

## 导出与维护

在联网开发机导出（两种剖面同一条命令，只差 `--arch` 与 SDK 目录）：

```bash
python3 packaging/gitea/prepare-bundle.py --arch amd64 \
    --sdk ../Rust-builder/kylin-x86/offline \
    --supplement-prebuilt /path/to/prebuilt \
    --output /media/<disk>/gitea-x86
python3 packaging/gitea/prepare-bundle.py --arch arm64 \
    --sdk ../Rust-builder/kylin-arm/offline \
    --supplement-prebuilt /path/to/prebuilt \
    --output /media/<disk>/gitea-arm64
```

`--supplement-prebuilt` 目录需含 `wunder-supplement-linux-<arch>.tar.gz` 与同名 `.sha256`（导出时逐包复核，
不匹配直接拒绝），准备方法见 `VALIDATION.md` 与 `cache/README.md`。脚本按剖面校验 SDK 必需目录、构建任务镜像、
流式 gzip 导出镜像与 SDK 归档，
镜像版本/ID/大小/SHA256 记在 `images/manifest.json`，SDK 与补充包缓存校验和记在 `rust-builder/SHA256SUMS`。
仅更新配置或镜像时可加 `--reuse-sdk-archive` 复用已校验的归档（不会重新收集 SDK 变更）。

固定 Gitea 28.2.0 与 Gitea Runner 5.2.3，不使用漂移的 latest 标签。
`examples/` 是工作流与 CI 脚本副本；其他项目应按自身依赖制作专用任务镜像。
已验证范围与未验证边界见同目录 `VALIDATION.md`。
