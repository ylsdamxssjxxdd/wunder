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

## 边界与未验证

- `.run` 安装包与便携包不在离线流水线里组装（`pack` 需要桌面、CLI、补充包同地，跨任务传递未接入），
  README 已注明用 `bash build.sh pack -a linux-<arch>` 在准备机上组包。
- `kit-arm64` 仍是 10-28 导出的上一代套件：单一 `linux-offline-arm64` 标签、无补充包任务、脚本早于本轮改动。
  要在 ARM 机器上用本轮设计，需 `prepare-bundle.py --arch arm64` 重新导出，并先确认 arm64 剖面的
  SDK 必需目录与 host-tools 短路条件成立。
- rcho 的三目标结论来自 `--network none` 的容器实测，还不是真实 Actions 运行：它的 `publish.py`/`prune.py`
  依赖任务级短期令牌，只有推送后在 Gitea 里才拿得到，因此发布与留存链路尚未在本机验证过；
  三个矩阵任务共用 `runner_capacity` 的真实并发调度也未实测。
- 补充包内容跟随 `packaging/python/requirements-linux-common.txt`：该文件变更后必须在准备机重产
  `prebuilt/` 并重新导出套件，否则离线端复用的仍是旧包（CI 不会自动发现）。
- 全量 Release 的构建耗时与缓存复用只在准备机这一台机器上量过；换硬件需重估 `runner_capacity` 与
  `WUNDER_BUILD_JOBS`。
