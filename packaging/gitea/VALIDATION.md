# 离线部署验证记录

日期：2026-11-02。准备机与验证机是同一台 x86_64 Linux（24 核 / 62G，Docker Engine + Compose v2）；
ARM64 剖面同日未在 ARM 机器上重跑，见文末边界。

## 架构

- Gitea、Runner、任务镜像三个镜像单独导出，`images/manifest.json` 与 `SHA256SUMS` 记录版本、ID、大小与摘要。
- Rust-builder 作为独立的 `rust-builder/offline-sdk.tar.gz` 交付，不复制进任何镜像；宿主机解压后由绑定命名卷
  只读挂到 `/opt/offline-sdk`，Runner 与每个任务容器共用同一挂载。编译缓存走独立可写卷。
- 补充包（opt/python、opt/git、opt/rg、opt/ffmpeg、opt/browser）在联网准备机产出，随套件以
  `supplement-prebuilt.tar.gz` 导出；离线端不重建，CI 只做 SHA256 与包内组件结构校验后复用发布。
- 剖面由 `prepare-bundle.py --arch {arm64,amd64}` 决定：平台、任务镜像、SDK 必需目录、卷/网络前缀、
  Runner 容量与线程数。工作流与 CI 脚本两种剖面共用。

## 已验证

导出与部署：

- `prepare-bundle.py --arch amd64` 导出成功；SDK 归档 93775 个条目，含 x86_64 与 aarch64 工具链、
  nightly + rust-src（Win7 build-std 用）、双架构 Ubuntu 18.04 sysroot、host-tools-amd64（带 `.complete`）、
  MinGW DW2 unwinder 与 `runtime-x86_64`/`runtime-aarch64`；`cargo-vendor-slint` 74321 条目。
- `load-images.sh` 按 `.env` 的 `CI_PLATFORM` 校验宿主架构后加载三个镜像，`sha256sum --check` 全部通过；
  `prepare-sdk.sh` 识别 `kylin-x86` 剖面、解压 SDK 与预制补充包、写回绝对路径。
- 整套目录搬迁到别处后：`prepare-sdk.sh` 自动从失效的旧路径回落到套件内目录，比对命名卷的 `device`
  并重建绑定卷，`docker compose up -d` 恢复正常；Gitea healthy、bootstrap 建管理员与 Runner 令牌、
  Runner 自动注册 `wunder-offline` 与 `rcho-offline`。
- 注册开关与登录可见性已改为 `.env` 可调（`GITEA_DISABLE_REGISTRATION`、`GITEA_REQUIRE_SIGNIN_VIEW`），
  置 false 后 `/user/sign_up` 可达、登录页出现注册入口。

真实项目全链路（wunder，仓库 HEAD 8ba2a9c + 套件脚本，单次 push 触发）：

- `linux (amd64)`、`linux (arm64)`、`win7`、`supplement (amd64)`、`supplement (arm64)` 五个任务全部成功。
  arm64 与 win7 都由 x86_64 宿主交叉完成，未使用 QEMU。
- 一次 Release 收齐 14 个附件：`wunder-slint-0.3.0-linux-{amd64,arm64}.tar.gz`（AppDir）、
  `wunder-frontend-slint-0.3.0-linux-amd64`、`wunder-cli-0.3.0-linux-{amd64,arm64}`、
  `wunder-frontend-slint-0.3.0-win7-x86.exe`、`wunder-cli-0.3.0-win7-x86.exe`、
  `wunder-supplement-linux-{amd64,arm64}.tar.gz` 及其 `.sha256`、三份 `SHA256SUMS-<arch>.txt`。
- `verify.py` 通过：Linux 侧 ELF 机器码 + CLI glibc ≤ 2.27 + AppDir 布局；Win7 侧 PE i386 头。
  win7 的 PE 导入审计转储不再混进发布目录。
- 补充包任务的实际日志证据是一行 `Supplement reused from the bundled prebuilt pack:
  wunder-supplement-linux-amd64.tar.gz (372152875 bytes)`，SHA256 校验先通过才发布。
- 任务容器只挂在 `internal` 网络上，整条流水线没有任何外网出口；未出现 `actions/checkout`、
  `upload-artifact`、`rustup`、`apt`、`npm` 等联网步骤。

隔离与只读性质的独立复核：

- 直接 `docker run --network none` 挂只读 SDK 跑 `ci/build.sh`（ARCH=amd64）：16 分 4 秒完成桌面 + CLI，
  `PASS: amd64 ELF headers, AppDir layout and CLI glibc compatibility`，全程联网尝试计数为 0。
- 准备机侧的补充包断网构建：amd64 与 arm64 均在 `--network none` 下成功，5 项 downloads 缓存全命中，
  pip 走 `file://` 的 PEP 503 目录索引；两架构 ELF 架构校验通过。
- 同一套件的第二条流水线（rcho，走 `rcho-offline` 标签 + 只读 SDK 的符号链接视图）在
  `--network none` 下三个目标全部通过：win7-x86 交叉 2 分 59 秒、Linux amd64 原生 3 分 26 秒、
  Linux arm64 由 x86_64 宿主交叉 3 分 52 秒（无 QEMU），门禁为 ELF 机器码 + 自解压载荷布局 +
  glibc ≤ 2.27 与 Win7 的 PE 导入/头校验。这轮复跑还暴露并修好一个残留产物问题（见下）。

发布物与 GitHub 同形这一轮（2026-11-03）：

- rcho 的流水线已在真实 Actions 里跑完：同一运行里 amd64、arm64、win7-x86 三个任务全部成功，
  Release `ci-<提交短哈希>-<运行号>` 收到 `rcho-0.2.0-amd64.appimage`、`rcho-0.2.0-arm64.appimage`
  与两份校验清单，说明 `publish.py` 的任务级令牌链路在离线实例上成立。
- Linux 侧改为桌面归档 + `.portable` + `.run` 三件：`builders/pack-portable.sh` 与 `builders/pack.sh`
  都在同一任务的工作区里跑，补充包直接取套件预置包（先 `sha256sum --check --strict`）；
  裸 CLI 转存进缓存卷，只作架构与 glibc 样本，不再当附件。
- Win7 安装包用第四个镜像（Ubuntu 22.04 + Wine + Inno Setup 6.7.3，版本与校验和同
  `build_win7_installer.ps1` 一致）。`packaging/windows/scripts/build_win7_installer_linux.py` 按
  PowerShell 版逐条复刻 [Tasks]/[Files]/ISCC 参数；实测在套件镜像里以 root 跑完整包：
  六个组件全部识别（python/git/rg/ffmpeg/powershell/browser），21 秒组出
  `wunder-0.3.0-win7-x86-setup.exe`（335,613,621 字节），`verify.py` 判为 i386 PE + Inno 载荷。
  另用小样本做了一次静默安装烟测：装完落出 `bin/wunder-desktop.exe`、`bin/wunder-cli.exe`、
  `.wunder-home`、卸载器与 `desktop.cmd`，未勾选的组件按预期不安装。
- win7 任务把桌面与 CLI 的 PE 写进 `/ci-cache/<仓库键>/win7-x86/installer-inputs/` 交给安装包任务，
  两个任务不共享工作区，靠同一个缓存卷交接。

## ARM64 剖面重导出（2026-11-03）

- arm64 剖面按本轮代际重新导出：三个标签（`wunder-offline`、`rcho-offline`、`wunder-offline-installer`
  指回构建镜像）、四个 CI 脚本齐备（含 `is-latest.py`、`prune.py`、`build-win7-installer.sh`）、
  `supplement-prebuilt.tar.gz` 随套件导出（amd64/arm64/win7-ia32 三份，导出前逐包复核 SHA256）。
- 老 Docker 的兼容件保留并回到仓库里成为模板：`compose.v1.yaml`（`version: "2.4"` + 卷与网络 `external: true`，
  前缀由导出脚本渲染成真实键名）、`upgrade-docker.sh`（麒麟 V10 静态二进制升级，带 `--check`/`--rollback`）、
  Runner 固定 `gitea/act_runner:0.2.11`（客户端下限 API 1.24），`.env` 新增 `RUNNER_MIN_DOCKER_API`
  并由 `load-images.sh` 在载入前拦截低于下限的 daemon。`load-images.sh` 现在两种 Compose 都能用，
  载入后逐镜像核对架构，并预建 `<前缀>-sdk/-build-cache/-ci/-ingress`。
- arm64 的 SDK 归档**未改动**（SHA256 仍是 10-28 那份），因此任务镜像只补了一层 `COPY ci /opt/offline-ci`
  重新种一个新 tag；构建时用 `--platform linux/arm64 --provenance=false`，避免 attestation 清单
  把导出变成多平台索引（`docker save/inspect --platform` 与老 daemon 都会出问题）。
- ARM 侧的构建能力是逐项查出来的，不是照抄 x86：aarch64 稳定版工具链里带着 `rustlib/x86_64-unknown-linux-gnu`
  （amd64 目标 std 齐备），`linux-amd64-ubuntu18/root/usr/bin/x86_64-linux-gnu-gcc-7` 实测是 aarch64 宿主 ELF
  并带 `strip/readelf/ar`，amd64 sysroot 里有 `libasound.so`；win7 走仓库既有的 ARM 通道
  （`build-win7-arm64-offline.sh` + `nightly-2026-03-14-aarch64` 的 rust-src + stamp
  `rcho-win32-arm64-portable-v2` 的 MinGW 便携包 + `llvm-readobj` 门禁）。wunder 当前 `Cargo.lock`
  需要的 crate 在 arm SDK 的 vendor 里齐全（该 vendor 1928 项，比 x86 SDK 的 1181 项还多）。
- CI 分派用桩 builder 复跑过：把宿主伪装成 aarch64 后 `build.sh` 选中 `build-win7-arm64-offline.sh` 与
  `build-cli-win7-arm64-offline.sh`，用真实 i386 PE 走完整链，`PASS: win7-x86 PE headers (i386)`，
  交接目录里同时落下桌面与 CLI 两个 exe。`prepare-sdk.sh` 的 kylin-arm 分支（新增 nightly rust-src、
  x86_64 目标 std、MinGW stamp、llvm-readobj、libgcc_eh.a、cargo-home-* 检查）对着真机 SDK 跑通，
  补充包解出到 `offline/supplement-prebuilt/`。

## 边界与未验证

- 旧的排队运行只靠任务自查让路（`ci/is-latest.py`），已经在跑的任务不会被打断；这个版本没有
  `concurrency`，也没有可脚本化的取消接口，界面上的 Cancel 仍是唯一的手动收口。
- Wine 组出的 setup.exe 没有在真实 Windows 7/10 机器上装过（本机无 Windows 宿主），证据只有
  PE 头、Inno 载荷标记与 Wine 下的静默安装；对外正式交付仍以 GitHub 的 Windows runner 产物为准。
  ARM worker 上没有可用的 i386 Wine，`win7-installer` 任务在该类机器上打印 SKIP 后成功退出，
  所以 arm64 剖面把该标签指向构建镜像本身，不留未注册的标签。
- arm64 剖面的**真机验收还没做**：本机是 x86_64，arm64 容器只能靠 qemu-user 执行，而 qemu-user 下
  `uname -m` 返回宿主架构（aarch64 的判定会失真），所以架构分派只能用桩 `uname` 复现，全量 cargo 构建、
  `-Z build-std` 与 AppDir 组包的真实耗时必须在 ARM 服务器上看 Actions 状态。
- `gitea/act_runner:0.2.11` 跑本轮工作流（matrix + needs + 任务级令牌发布 + `prune.py`）尚未在真实运行里验证；
  它跑过的是上一代 rcho 三目标矩阵。老 runner 会把带表达式的步骤名显示成 `%!t(string=...)`，只影响显示。
- ARM 侧的 Release 比 x86_64 侧少一个 `wunder-<版本>-win7-x86-setup.exe`：Inno Setup 要在 Wine 里跑 i386
  的 `ISCC.exe`，aarch64 宿主没有可用的 i386 Wine，`win7-installer` 任务按架构打印 SKIP（成功退出，不影响
  其余任务）。桌面上线要出这个安装包，仍以 x86_64 那一侧的运行为准。
- rcho 的三目标结论来自 `--network none` 的容器实测，还不是真实 Actions 运行：它的 `publish.py`/`prune.py`
  依赖任务级短期令牌，只有推送后在 Gitea 里才拿得到，因此发布与留存链路尚未在本机验证过；
  三个矩阵任务共用 `runner_capacity` 的真实并发调度也未实测。
- 补充包内容跟随 `packaging/python/requirements-linux-common.txt`：该文件变更后必须在准备机重产
  `prebuilt/` 并重新导出套件，否则离线端复用的仍是旧包（CI 不会自动发现）。
- 全量 Release 的构建耗时与缓存复用只在准备机这一台机器上量过；换硬件需重估 `runner_capacity` 与
  `WUNDER_BUILD_JOBS`。
