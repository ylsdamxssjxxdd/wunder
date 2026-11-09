# 离线 Gitea 与双架构 CI（arm64 / x86_64 剖面）

本包面向已有 Docker Engine 与 Docker Compose 的 Linux 服务器，一套脚本按 `--arch` 导出两种剖面：
`arm64`（ARM64 宿主，amd64 与 Win7 32 位都靠 SDK 内的交叉工具链原生完成）与
`amd64`（x86_64 宿主，arm64 与 Win7 32 位同样靠交叉）。两者都不需要 QEMU，都保持 Ubuntu 18.04 / glibc 2.27 基线。
桌面版（wunder）两个剖面都出 Linux 桌面归档 + `.portable` + `.run` 与 Win7 的桌面/CLI PE；
只有 Win7 的 Inno Setup 安装包必须在 x86_64 上用 Wine 组，ARM 剖面会跳过那一个任务。
arm64 剖面默认配老版 Runner（`gitea/act_runner:0.2.11`）与 `compose.v1.yaml`，专为只有老 Docker Engine 的机器（如麒麟 V10 的 18.09）。

## 启动

1. 把整个目录复制到服务器的可写目录，保留 `compose.yaml`、`images/`、`rust-builder/` 的相对位置。
2. 执行 `bash load-images.sh`。它先按 `.env` 的 `CI_PLATFORM` 校验宿主架构（x86_64 套件不能在 ARM 机上装，反之亦然），
   再校验 daemon 的 API 版本是否满足 `.env` 的 `RUNNER_MIN_DOCKER_API`，然后校验并加载 `images/` 里的三或四个镜像
   （x86_64 剖面多一个 Wine 安装包镜像），接着校验、解压 `rust-builder/offline-sdk.tar.gz` 与
   `rust-builder/supplement-prebuilt.tar.gz`，自动把 SDK 的宿主机绝对路径记进 `.env`，最后预建固定名字的网络与卷。
   不需要手工改 Runner 挂载路径。
3. 编辑 `.env`，把 `GITEA_PUBLIC_URL` 改成浏览器可访问的地址（如 `http://<服务器地址>:3000/`）；
   `GITEA_SSH_HOST` 只填主机名或地址。HTTP/SSH 端口默认 3000/2222。
4. 起服务：`docker compose up -d`（Compose v2），只有 Python 版 `docker-compose` 1.29.x 的机器改用
   `docker-compose -f compose.v1.yaml up -d`。镜像策略为 `never`，不会访问外部镜像仓库；
   初始化、管理员创建、Runner 注册自动完成。
5. `sudo cat data/bootstrap/admin-password` 取初始密码登录，首次登录必须改密码。

```bash
docker compose ps -a
docker compose logs --tail=100 gitea bootstrap runner
```

默认关闭自助注册与外部头像，私有仓库需登录。HTTP 不加密，仅用于可信内网。

### 只有老 Docker / Compose v1 的机器

`compose.yaml` 用的是 v2 语法（顶层 `name:`、`pull_policy`、卷与网络的 `name:`）。老机器上有两条路：

- **不升级引擎**：用 `compose.v1.yaml`（`version: "2.4"`，卷与网络写成 `external: true`，由 `load-images.sh` 预建）。
  v1 的键不能用 `${VAR}` 插值，所以文件里的前缀由 `prepare-bundle.py` 渲染成真实值；
  v1 没有 `pull_policy: never`，但镜像已在本地时 `up` 不会拉取，缺镜像才会失败——所以别跳过 `load-images.sh`。
  Runner 也要跟着降版本：`docker.gitea.com/runner:5.2.3` 的内置客户端要求 daemon API ≥ 1.40，
  在 18.09（API 1.39）上任务会在启动容器时报 `client version ... is too new`（Gitea 本体一切正常）；
  arm64 剖面因此默认 `gitea/act_runner:0.2.11`（下限 API 1.24），`load-images.sh` 按 `.env` 的
  `RUNNER_MIN_DOCKER_API` 提前拦住。已知差异：老 runner 会把带表达式的步骤名显示成 `%!t(string=...)`，只影响显示。
- **升级引擎**：`sudo bash upgrade-docker.sh --package /path/docker-24.0.9.tgz --check` 先体检，去掉 `--check` 执行；
  用静态二进制包替换，绕开发行版软件源，失败自动回滚（`--rollback /var/backups/docker-upgrade-<时间戳>`）。
  24.0.9 的 API 是 1.43，高于新版 Runner 的 1.40 下限，升完可以直接改用 `docker.gitea.com/runner:5.2.3`。
  注意 `docker load` 的归档是 OCI 格式并保留 legacy `manifest.json`，18.09 也能载入。
- **android arm64 与老 seccomp 的冲突**：套件里的 `qemu-x86_64-static` 是 Ubuntu 22.04 的静态构建（自带
  glibc 2.35），启动建线程走 `clone3`；Docker 18.09/19.03 的默认 seccomp 对 `clone3` 直接返回 EPERM，
  任务会在第一次调用 NDK clang 时 abort（`qemu: qemu_thread_create: Operation not permitted`）。
  arm64 剖面因此给任务容器加了 `--security-opt seccomp=unconfined`；升到 Docker ≥20.10.10 后默认 profile
  对 `clone3` 返回 ENOSYS（glibc 回退到 `clone`），可以把 `runner.yaml` 里这个选项删掉恢复默认沙箱。
  amd64 剖面不用 qemu，不受影响。

## 剖面、标签与挂载

`.env` 里由导出脚本写入的剖面参数：`CI_PLATFORM`、`SDK_PROFILE`（kylin-arm / kylin-x86）、
`STACK_PREFIX`（卷与网络前缀，arm64 为 `offline-gitea`，x86_64 为 `offline-gitea-x86`，两套可在同一台机器并存）、
`GITEA_IMAGE`、`RUNNER_IMAGE`、`BUILDER_IMAGE`、`RUNNER_LABELS`，以及 `GITEA_DISABLE_REGISTRATION`/
`GITEA_REQUIRE_SIGNIN_VIEW`（套件默认关闭自助注册并要求登录，个人机器可置 false 放开）。

Runner 注册三个标签，前两个指向同一个任务镜像，只是给两个仓库各留一条路由：
- `wunder-offline` → 任务镜像（Ubuntu 18.04 + Rust/Slint 交叉工具链 + git + `/opt/offline-ci`），构建 Linux 桌面/CLI、Win7 32 位交叉，并复用预制补充包。
- `rcho-offline` → 同一个镜像，供 rcho 的离线流水线使用。rcho 仓库自带 `.gitea/workflows/offline.yml` 与 `packaging/gitea/ci/`，推到同一实例即可构建 Linux x64/arm64 与 Win7 x86；检出前只能调用镜像里烘焙的 `/opt/offline-ci/checkout.py`，检出后一律改用仓库内脚本。因为 rcho 的构建器会往 SDK 的 `cargo-home-*` 写配置，它的 `prepare-view.sh` 把只读 SDK 组织成 `/ci-cache` 下的可写视图（只读内容用符号链接引用），任何情况下都不向 `/opt/offline-sdk` 写入。
- `wunder-offline-installer` → x86_64 剖面指向带 Wine + Inno Setup 的第四个镜像；arm64 剖面没有可用的 i386 Wine，该标签指回构建镜像，任务按宿主架构自行跳过。标签名必须留着，否则 `runs-on: wunder-offline-installer` 排不到 runner、任务永久排队。

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

- `linux`：矩阵 arm64/x64。桌面 `wunder-desktop-<版本>-linux-<x64|arm64>.tar.gz`、便携包 `wunder-desktop-<版本>-linux-<x64|arm64>.portable`、
  安装包 `wunder-desktop-<版本>-linux-<x64|arm64>.run`；裸 CLI 只作架构与 glibc 门禁的样本，组完包转存到缓存卷，不当附件发布。
  `verify.py` 校验 ELF 架构、AppDir 布局、桌面二进制的 glibc ≤ 2.27，以及两个自解压件的头部标记与 gzip 载荷。
- `win7`（两种宿主都真构建）：`wunder-desktop-<版本>-win7-x86.exe`，`verify.py` 校验 PE i386 头；
  同时把桌面与 CLI 两个 PE 写进 `/ci-cache/<仓库键>/win7-x86/installer-inputs/` 交给下一个任务。
  宿主架构决定走哪套通道：x86_64 用 `build-win7-x86-offline.sh`（kylin-x86 的 MinGW 便携包），
  aarch64 用 `build-win7-arm64-offline.sh`（SDK 内 aarch64 宿主版便携包 + `nightly-*-aarch64` 的 rust-src 走
  `-Z build-std`），两条通道的产物名、`imports.txt` 与 Win7 导入门禁一致。
- `win7-installer`（`needs: win7`）：在带 Wine + Inno Setup 的第四个镜像里跑 `ISCC.exe`，用交接 PE 与预置的
  `wunder-supplement-win7-ia32-common.zip` 组出 `wunder-desktop-<版本>-win7-x86-setup.exe`。ARM 宿主跑不了 i386 Wine，
  脚本按 `uname -m` 打印 SKIP 并成功退出（所以 arm64 剖面把该标签指向构建镜像本身，不留空标签）——
  ARM 侧的 Release 与 x86_64 侧只差这一个安装包。
- `android`（x86_64 或 aarch64 worker）：`wunder-android-<版本>-arm64.apk`，内嵌 proot Ubuntu rootfs、离线
  工具补充包 `common.tar.xz` 与 proot/busybox 运行库。工具链（Android SDK 36、NDK 28.2、Temurin JDK 17、
  Gradle 8.13 与预热缓存）和 APK 资产放在套件 `android/` 目录，由 `<前缀>-android` 卷只读挂载到任务容器的
  `/opt/android`。x86_64 宿主直接用 NDK 原生工具链；aarch64 宿主用套件内置的 `qemu-x86_64-static` 模拟
  x86_64 的 clang/aapt2（Google 只为 x86_64 宿主发布 NDK 与 build-tools），Rust 仍按 aarch64 原生编译。
  Gradle 缓存在首次构建时复制到 `/ci-cache` 后复用，调试签名固定使用 `android/keystore/debug.keystore`，
  多次构建的 APK 可互相覆盖安装。

检出后的步骤一律使用仓库内 `packaging/gitea/ci/` 脚本，只有检出本身用镜像内 `/opt/offline-ci/checkout.py`。
产物由该次运行的短期令牌上传到 Releases，标签形如 `ci-<提交短哈希>-<运行编号>`。
某个任务失败不影响其他任务；Release 有附件不代表全部成功，以 Actions 任务状态为准。

这个版本的 Gitea 没有工作流 `concurrency`（二进制里找不到 `cancel-in-progress` 的实现），服务端也不会
替新提交取消旧运行，所以连续推送会排着队各跑一遍。改由任务自己判：`ci/is-latest.py` 用本次的短期令牌
`git ls-remote` 取分支头，头已经不是本次提交就打印「已被取代」并让任务立刻成功退出（不构建、不发布）。
标签推送与手工触发不受影响；查询失败一律放行，宁可多跑一次也不把流水线卡死。注意它只能让**还没开始**的
任务让路，已经在编译中的任务不会被打断。

## 离线边界与资源

- Rust crate vendor、双架构工具链与 sysroot、MinGW、AppImage runtime 由 SDK 目录只读挂载。
- 安卓工具链与 APK 内嵌资产由套件 `android/` 目录只读挂载到 `/opt/android`，准备方法：
  ```bash
  packaging/gitea/prepare-android.sh --out /path/to/kit/android \
    --sdk "$HOME/android-sdk" --jdk /path/to/temurin-17 \
    --gradle "$HOME/gradle/gradle-8.13" --gradle-home "$HOME/.gradle" \
    --rust-std /path/to/rust-std-1.92.0-aarch64-linux-android.tar.xz \
    --offline-sdk /path/to/kit/rust-builder/offline \
    --skia /path/to/skia-binaries
  ```
  ARM64 任务机把 `--jdk` 换成 aarch64 Temurin，并追加
  `--host-arch aarch64 --qemu /usr/bin/qemu-x86_64-static --x86-root /path/to/x86_64-root`：
  `x86_64-root` 是最小 glibc 运行库根，至少包含 `lib64/ld-linux-x86-64.so.2` 与
  `lib/x86_64-linux-gnu/{libc,libm,libdl,libpthread,librt,libgcc_s,libz}.so*`；脚本会在 `android/bin/`
  生成 clang/ar/aapt2 的 qemu 包装器和直接调 `ld.lld` 的 `rust-linker`。模拟环境下 aws-lc 走
  `AWS_LC_SYS_CMAKE_BUILDER=0` 的 cc 构建链，避免 CMake 链接探测（clang 驱动在 qemu 里无法 exec 子进程）。
  JDK 必须是 Temurin 17（glibc 2.17 基线）；发行版自带的 OpenJDK 17 往往要求 glibc 2.33+，在
  Ubuntu 18.04 任务镜像里起不来。Android 依赖复用 SDK 的 `cargo-vendor-slint`，但安卓 crate 有独立的
  `crates/wunder-android/Cargo.lock`（如 `jni`、`android-activity`），套件导出前必须把它 vendor 进同一快照：
  ```bash
  (cd crates/wunder-android && cargo vendor --locked --versioned-dirs /tmp/vendor-android)
  comm -23 <(ls /tmp/vendor-android | sort) <(ls "$OFFLINE_SDK/cargo-vendor-slint" | sort) |
    while read -r crate; do cp -a "/tmp/vendor-android/$crate" "$OFFLINE_SDK/cargo-vendor-slint/"; done
  ```
  合并后再重打 `rust-builder/offline-sdk.tar.gz` 与 `SHA256SUMS`，否则重新部署的套件会在解析阶段报
  `failed to select a version`。
  Slint 的安卓渲染链还会用 rust-skia 的预编译包（`skia-bindings 0.153.3` 的
  `aarch64-linux-android-ganesh-gl-…` 变体），准备机联网下载后放到
  `android/skia-binaries/<版本>/skia-binaries-<key>.tar.gz`；`ci/build-android.sh` 用
  `SKIA_BINARIES_URL=file://…/{tag}/skia-binaries-{key}.tar.gz` 指过去。skia 版本或特性变化时
  key 会变，以构建日志里 `TRYING TO DOWNLOAD AND INSTALL SKIA BINARIES` 那一行为准。
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
- 补充包依赖（requirements-linux-common.txt）或工具链变化后同样要重产重导。x86_64 剖面默认 5 个并行任务、每任务 4 线程（建议 ≥24 核、64 GB 内存、200 GB 可用存储）；
  arm64 剖面默认 1 并行、4 线程。改 `prepare-bundle.py` 的 `runner_capacity`/`build_jobs` 后重新导出。
- 数据在 `data/gitea/`（仓库、账号、SQLite、Release 附件），Runner 注册在 `data/runner/`，
  启动凭据在 `data/bootstrap/`。备份先 `docker compose stop` 再拷 `data/`。发布留存由仓库工作流负责：
  每个发布步骤在 `publish.py` 之后跑 `ci/prune.py`，按 `KEEP_CI_RELEASES`（示例工作流设 5）只留最近的
  `ci-*` 发布并连带删除其附件与标签，手工 `v*` 标签永不触碰；上限是**每个仓库**各自计算，
  且只在有产物发布的任务里执行。wunder 一次运行约 1.4–1.8 GB（ARM 剖面无 setup.exe，落在 1.4 GB 上下），
  默认留存约 7–9 GB；要更紧就把 `KEEP_CI_RELEASES` 改小。
- 套件本身的体积：arm64 的 `rust-builder/offline-sdk.tar.gz` 约 1.3 GB、`supplement-prebuilt.tar.gz` 约 1.1 GB、
  镜像约 350 MB；解压后 SDK 约 5.4 GB。首次 Rust 全量构建在 8 核 ARM 上耗时明显，之后复用
  `<STACK_PREFIX>-build-cache`（实测该卷会涨到几十 GB，清理它等于丢增量编译，不影响发布数据）。

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

在 x86_64 准备机上导出 arm64 剖面时要加 `--skip-build`（`Dockerfile.builder-arm64` 的基镜像与 apt 源都在
arm 侧准备机上），任务镜像从上一版套件 `docker load` 回来后再补一层 `COPY ci /opt/offline-ci`：

```bash
docker load --input kit-arm64/images/builder-arm64.tar.gz
printf 'FROM wunder-offline-builder:20261027-arm64-mounted\nCOPY ci /opt/offline-ci\n' \
  | DOCKER_BUILDKIT=1 docker build --platform linux/arm64 --provenance=false \
      -t wunder-offline-builder:20261103-arm64-mounted -f - packaging/gitea
```

`--provenance=false` 是必须的：带 attestation 的构建会把镜像导出成多平台清单，`docker save/inspect --platform`
与老 daemon 都会出问题。gitea 与 runner 的 arm64 归档同理从旧套件载入，导出脚本只用 plain `docker save`，
并核对镜像配置里的架构与剖面一致。

固定 Gitea 28.2.0；Runner 按剖面固定——amd64 用 `docker.gitea.com/runner:5.2.3`，
arm64 用 `gitea/act_runner:0.2.11`（老 daemon 兼容），都不用漂移的 latest 标签。
改 Runner 版本要同步 `.env` 的 `RUNNER_IMAGE`/`RUNNER_MIN_DOCKER_API` 与 `images/` 里的归档。
`examples/` 是工作流与 CI 脚本副本；其他项目应按自身依赖制作专用任务镜像。
已验证范围与未验证边界见同目录 `VALIDATION.md`。
