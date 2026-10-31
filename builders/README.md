# Desktop and CLI builders

本目录集中放置桌面 Slint 与 CLI 的离线发布、交叉编译和分发门禁脚本。仓库根目录只保留两个公开入口：Linux 使用 `build.sh`，Windows 使用 `build.bat`。`frontend-slint/scripts/` 只保留原生联调与启动回归。

公开入口统一以程序目标和分发架构选择构建，并按宿主自动选择离线 SDK：ARM64 Linux 使用 `Rust-builder/kylin-arm`，x86_64 Linux 使用 `Rust-builder/kylin-x86`（准备方法见“kylin-x86”章节）。推荐使用简拼 `-t`/`-a`，`-target`/`-arch`/`--target`/`--arch` 写法等价：

```bash
# 两种 Linux 宿主都可直接执行；x86_64 上 linux-amd64 原生构建，
# linux-arm64 与 win7-x86 由该 SDK 的交叉工具链构建。
bash build.sh -t desktop -a linux-amd64
bash build.sh -t cli -a linux-amd64
bash build.sh -t desktop -a linux-arm64
bash build.sh -t cli -a linux-arm64
bash build.sh -t desktop -a win7-x86
bash build.sh -t cli -a win7-x86

# Linux Desktop（arm64 与 amd64）产出裸 ELF 和同目录的 AppDir 目录归档
# （wunder-slint-<ver>-linux-<arch>.tar.gz），.run 安装包与便携包都从该归档
# 解压安装，目标机无需 FUSE/libfuse2；CLI 永远是普通 ELF。

# 构建当前 Linux 主机可发布的全部程序：三个架构 × Desktop/CLI。
bash build.sh -all

# 打出免安装便携包（单文件，首次启动自解压到 ~/.wunder，无需 FUSE）。
bash build.sh portable -a linux-amd64
```

`--docker` 固定使用 ARM64 Ubuntu 18.04 构建镜像并读取 `Rust-builder/kylin-arm`，与宿主架构无关；ARM64 宿主的默认 `--native` 走 kylin-arm，x86_64 宿主走 kylin-x86。

Windows 使用同一套目标/架构名称。Windows 上的 Linux 目标通过 Docker 使用 `kylin-arm`；`win7-x86` 在 Windows 上通过 `win7` SDK 本地构建，Linux 宿主用 `build.sh` 交叉生成（ARM64 与 x86_64 均可，Desktop 与 CLI 同一 `i686-win7-windows-gnu` 目标，build-std 方式）。旧的 `win32-x86` 名称已并入 `win7-x86`（Win7 兼容构建覆盖全部 32 位 Windows 场景）：

```bat
build.bat -t desktop -a win7-x86
build.bat -t cli -a linux-amd64
build.bat -All
```

## 打包安装器（pack）

`pack` 只组合已经构建好的产物，不会重新编译：

```bat
build.bat pack -a win7-x86
build.bat pack -a linux-amd64
build.bat pack -a linux-arm64
```

```bash
bash build.sh pack -a linux-amd64
bash build.sh pack -a linux-arm64
```

Windows 安装包优先使用离线 SDK 中的 `Rust-builder/win7/offline/tools/inno-setup/app/ISCC.exe`（Inno Setup 6.7.3，随 SDK 分发，版本与 SHA256 见同目录 `VERSION.txt`）；也可以用 `-Iscc` 或 `WUNDER_ISCC` 覆盖，`-NoDownload` 禁止联网。只有离线编译器缺失且允许下载时，才会准备到 `temp_dir/tools/inno-setup`。SDK 根目录可用 `WUNDER_WIN7_BUILDER_ROOT` 或 `WUNDER_BUILDER_ROOT` 覆盖。

Linux 免安装便携包由 `build.sh portable` 生成：`packaging/linux/portable/portable-header.sh` 是 POSIX shell 启动器，`builders/pack-portable.sh` 把渲染后的启动器与 AppDir 目录归档直接拼成 `wunder-<ver>-<x64|arm64>.portable`。首次启动解压到 `<home>/app/desktop` 并写入 `.wunder-home`，之后按 `.portable-version` 跳过解压；布局与 `.run` 的安装结果完全一致，两者可互相替换且不会动用户数据。它不需要 FUSE、`libfuse2`、AppImage runtime 或 root 权限，是现代 Linux 上"下载即跑"的推荐形态。

Linux 安装包是单文件自解压 `.run`，由 Python 3.6+ 组装，不需要 Docker 或 Rust 工具链，也不访问网络。Windows 上组包时优先复用 SDK 的 `Rust-builder/win7/offline/tools/python/python.exe`（Python 3.8.10 embeddable），因此宿主机未安装 Python 也能离线生成 `.run`；Linux 宿主使用系统 `python3`。

输入默认从构建产物目录查找：

- 蛹：`target/frontend-slint/dist/win7-x86/wunder-frontend-slint-<ver>-win7-x86.exe`；Linux 优先使用 `target/slint/dist/linux-<arch>/wunder-slint-<ver>-linux-<arch>.tar.gz`（AppDir 目录归档，安装时解压，无需 FUSE）
- 舵机：`target/cli/dist/<target>/wunder-cli-<ver>-<target>[.exe]`
- 补充包：`dist/<target>/`、`target/installer/payload/<target>/`、`temp_dir/win7-gnu-lab/win7-supplement/dist/`、`temp_dir/linux-supplement/dist/`。找不到或匹配到多个时用 `-Supplement`/`--supplement` 指定。Windows 的 `-Supplement` 也接受已解开的目录（其下直接有 `opt/<component>` 和 `wunder-win7-supplement.json`），可复用补充包构建产物并跳过首次 zip 解压。

`pack` 会校验版本、PE/ELF 架构和补充包的 `opt/<component>` 结构，输出到 `target/installer/dist/<target>/`，并生成 `.sha256` 与 `.manifest.json`。`-CoreOnly` 只生成不含补充组件的安装器，便于快速验证。

安装器默认目录为 `%USERPROFILE%\.wunder`（Windows）或 `~/.wunder`（Linux）；Windows 安装向导可改到任意可写目录，Linux 用 `--home=PATH` 指定（交互模式只有一屏组件选择，回车即开始安装）。安装根会写入 `.wunder-home` 标记，蛹和舵机在启动最早期把 `WUNDER_HOME` 指向该根，因此配置、会话、工作区、日志、数据库和 `opt/` 补充组件都会跟着迁移；显式设置的非空 `WUNDER_HOME` 仍然优先。补充组件默认全部不勾选；Windows 组件页的“全选（推荐）”会一次勾选全部补充组件与桌面快捷方式，任一项取消会自动回退全选状态；选中已存在的组件会先清空对应 `opt/<component>`，用户数据不会删除。

## CI 发布

`.github/workflows/desktop-nightly.yml` 在每次分支推送时构建三套蛹和舵机，并从固定的 `supplement` release 下载对应补充包，再用 `builders/pack.ps1` / `builders/pack.sh` 组装安装包。`nightly` release 因此同时包含三个桌面程序和三个安装包：Win7 x86、Linux amd64、Linux arm64。打包前会等待同一提交触发的 Desktop Supplements 运行完成，避免旧补充包被打进安装包。

CI 使用的构建入口：

- Windows：`builders/build-win7-cli.ps1 -Toolchain <rustup工具链> -MingwBin <mingw32/bin> -HostMingwBin <mingw64/bin> -OutputDirectory <目录>`；不传 `-Toolchain` 时仍使用相邻离线 SDK。
- Linux：`builders/build-linux-cli-native.sh`，在 `packaging/docker/Dockerfile.ubuntu18-desktop` 容器内原生构建 amd64 或交叉构建 arm64；输出到 `target/cli/dist/linux-<arch>/`，可用 `WUNDER_CLI_OUTPUT_DIR` 覆盖。

egui 版本提供独立的本机快速构建入口，不改变默认 Slint Desktop 发布矩阵：

```bat
build.bat -Egui
```

Win7 x86 发布必须选择 Win7 目标，而不能把上面的本机 x64 产物复制到旧系统：

```bat
build.bat -Egui -a win7-x86
```

```bash
./build.sh --egui
```

本机快速构建产物位于 `target/wunder-egui/release/wunder-egui.exe`（Windows）或
`target/wunder-egui/release/wunder-egui`（Linux）。该入口使用当前构建主机的 native
Glow 后端，便于先运行 `--demo` 做视觉验收。Win7 x86 的产物位于
`target/wunder-egui/dist/win7-x86/wunder-egui-<version>-win7-x86.exe`，使用
`i686-win7-windows-gnu` 重编标准库。发布前会解析 PE 导入表，拒绝
`bcryptprimitives.dll`、`ProcessPrng`、`api-ms-win-*`、`WaitOnAddress` 等 Win7
不兼容项，并写出同目录的 `imports.txt`。Win7 目标依赖独立的
`Rust-builder/win7/offline/cargo-vendor-egui` 锁定快照；缺失依赖时构建应直接失败，
不得改用主机 Cargo 或手工复制系统 DLL。

`-all` 为完整发布构建 Linux ARM64、Linux amd64 与 Win7 x86 的 Desktop/CLI；Linux Desktop 产物为 AppDir 目录归档，`.run` 与 `-portable` 都从它解压，目标机不需要 FUSE；CLI 在所有目标都只产生普通 ELF/EXE。Windows 的 `-All` 默认分别使用相邻的 `Rust-builder/kylin-arm` 和 `Rust-builder/win7`；如需覆盖，使用 `-KylinBuilderRoot` 与 `-Win7BuilderRoot`，不要把两套 SDK 指到同一目录。

底层实现包括 `build-linux-*-*.sh`、`build-cli-*.sh`、`build-win7-*.ps1`、`build-win7-arm64-offline.sh`、`build-cli-win7-arm64-offline.sh` 和链接器辅助脚本。其中 `build-linux-desktop-native.sh` 是 CI 专用入口：在已预装 Ubuntu 18.04 工具链的容器内在线原生构建并打包 AppDir 归档，不依赖 kylin-arm 离线 SDK，打包语义与 `build-linux-arm64-desktop.sh` 保持同步。Win7 Windows 本机构建固定使用相邻的 `Rust-builder/win7/offline`；Linux ARM64 开发机与 Windows Docker 的 Linux 交叉构建固定使用相邻的 `Rust-builder/kylin-arm/offline`（Win7 交叉使用其中的 `nightly-2026-03-14-aarch64-unknown-linux-gnu` 工具链 + rust-src，-Z build-std 重编 std，MinGW 链接与导入库由 `mingw-i686-windows-gnu` 提供，sdk-version 戳接受 wunder/rcho 两种印记）。Linux 可通过 `WUNDER_BUILDER_ROOT` 覆盖 kylin-arm 根目录；Windows 分别通过 `-KylinBuilderRoot`、`-Win7BuilderRoot` 覆盖两套 SDK。脚本不探测或回退到旧 SDK 目录。

`prepare-linux-amd64-runtime-sysroot.sh` 是 `kylin-arm` 的一次性维护脚本，用于补齐 Linux amd64 Desktop 的 X11、XTest 与 ALSA 运行库。它不是日常构建步骤，必须在 **x86_64 Ubuntu 18.04** 环境执行，且只能将 SDK 挂载为可写；脚本固定使用 Ubuntu Bionic 归档源，下载完成后会把校验清单写入 SDK。常规 `build.sh`、`build.bat` 构建始终离线，不会调用它。例如：

```bash
docker run --rm --network host \
  -v /path/to/kylin-arm:/builder/kylin-arm \
  -v /path/to/wunder:/workspace:ro \
  -w /workspace ubuntu:18.04 \
  env WUNDER_BUILDER_ROOT=/builder/kylin-arm \
  bash builders/prepare-linux-amd64-runtime-sysroot.sh
```

### kylin-x86：x86_64 主机原生离线 SDK

没有 `kylin-arm` 源 SDK 时，可在 x86_64 主机上从零准备等价的 `Rust-builder/kylin-x86`，全部钉在 Ubuntu 18.04（glibc 2.27）：

```bash
# 1. 构建离线构建镜像（bionic + gcc-7/aarch64 交叉链/i686 mingw + 双 Rust 工具链）。
#    rustup 走 USTC rust-static 镜像（pinned nightly 只有它还镜像）。
docker build --platform linux/amd64 \
  -f packaging/docker/Dockerfile.rust-builder-kylin-x86 \
  -t wunder-rust-builder-kylin-x86:ubuntu18 .

# 2. 生成 amd64/arm64 sysroot（bionic 运行库闭包 + 开发头文件，含 deb 缓存）。
docker run --rm --network host --platform linux/amd64 \
  -v /path/to/wunder:/workspace:ro -v /path/to/Rust-builder/kylin-x86:/output \
  -w /workspace ubuntu:18.04 \
  env WUNDER_OFFLINE_ROOT=/output/offline \
  bash builders/prepare-kylin-x86-sysroots.sh

# 3. 从镜像导出工具链、host tools 与元数据。
docker run --rm --network none --platform linux/amd64 --entrypoint bash \
  -e OUTPUT_PROFILE=/output -v /path/to/Rust-builder/kylin-x86:/output \
  -v /path/to/wunder/builders:/scripts:ro \
  wunder-rust-builder-kylin-x86:ubuntu18 /scripts/export-kylin-x86-sdk.sh

# 4. cargo vendor（主 workspace + frontend-slint + rust-src 的 std 依赖三者并集；
#    cargo vendor 会清空目标目录，必须各自 vendor 到临时目录后合并）。
# 6. docker image save 到 offline/archives/builder-amd64-ubuntu18.docker.tar，
#    再跑 check-kylin-x86-sdk.sh 门禁与 index-kylin-x86-sdk.py 索引。
```

SDK 就绪后，x86_64 主机直接用根入口构建全部目标（无需 Docker）：

```bash
WUNDER_BUILD_JOBS=$(nproc) bash build.sh -t desktop -a linux-amd64   # 原生桌面归档
WUNDER_BUILD_JOBS=$(nproc) bash build.sh -t cli     -a linux-amd64   # 原生 ELF
WUNDER_BUILD_JOBS=$(nproc) bash build.sh -t desktop -a linux-arm64   # 交叉桌面归档
WUNDER_BUILD_JOBS=$(nproc) bash build.sh -t cli     -a linux-arm64   # 交叉 ELF
WUNDER_BUILD_JOBS=$(nproc) bash build.sh -t desktop -a win7-x86      # 交叉 PE
WUNDER_BUILD_JOBS=$(nproc) bash build.sh -t cli     -a win7-x86      # 交叉 PE
```

ARM64 交叉使用 `offline/host-tools-amd64/root` 中 x86_64 宿主的 aarch64 gcc-7 与 binutils（缺失时由 `prepare-x86-arm64-cross.sh` 从 `offline/archives/host-tools-amd64-ubuntu18.tar` 按需解包），目标用户的库来自 `linux-arm64-ubuntu18` sysroot；无需宿主机安装交叉编译器。

注意：离线脚本通过 `--config 'source.crates-io.replace-with="vendored-sources"'` 传
入 vendor 替换，因为 cargo ≥1.96 解析配置主目录依据 passwd 而非 `HOME`/`CARGO_HOME`，
开发者全局 `~/.cargo/config.toml`（镜像源等）会压过写入 cargo-home 的配置文件。

Win7 x86 同样可以在 x86_64 主机离线交叉构建（蛹与舵机，根入口见上）：

```bash
WUNDER_OFFLINE_ROOT=/path/to/Rust-builder/kylin-x86/offline \
WUNDER_BUILD_JOBS=$(nproc) bash builders/build-win7-x86-offline.sh
WUNDER_OFFLINE_ROOT=/path/to/Rust-builder/kylin-x86/offline \
WUNDER_BUILD_JOBS=$(nproc) bash builders/build-cli-win7-x86-offline.sh
```

这条链路依赖 `offline/mingw-i686-windows-gnu/` 的 portable MinGW：bionic 的
`i686-w64-mingw32-gcc` 是 SJLJ 展开，而 Rust 的 `i686-windows-gnu` std 需要 DW2 的
`_Unwind_Resume`，所以 rust-target/lib 里放 mingw-builds 7.3.0 posix-dwarf 的
`libgcc_eh.a`/`libgcc.a`，`portable-bin` 用重定位包装器（`-B` 前缀 + 捆绑
libisl/libmpc/libmpfr/libgmp）剔除 `-lgcc_eh/-lgcc` 后追加 DW2 存档和
`-lwinpthread`。`libsynchronization.a` 是空存档：winapi 0.3.9 的 synchapi 特性在
GNU 目标会请求它，Win7 下相关符号由 kernel32 提供，空存档即可通过链接且不引入
`api-ms-win-*` 导入。产物仍需通过 PE 导入表门禁。

准备完成后，`linux-amd64-ubuntu18/root` 必须包含 `libX11.so.6`、`libXtst.so.6`、`libasound.so.2`、`libasound.so`、`libxcb.so.1`、`libxcb-xkb.so.1`、`libxkbcommon.so.0` 和 `libxkbcommon-x11.so.0`；其中 `libasound.so` 只用于交叉链接，打包脚本验证其余运行库。

`prepare-linux-arm64-runtime-sysroot.sh` 是 arm64 侧的同类一次性维护脚本，把 bionic arm64 的 X11/XCB/XKB/ALSA 运行库闭包和 `squashfs-tools`（含压缩依赖）装进 `linux-arm64-ubuntu18/root`；必须在 **aarch64 Ubuntu 18.04** 环境（如 `rcho-slint-arm64-ubuntu18` 镜像）执行并将 SDK 挂载为可写。准备完成后，ARM64 Desktop 构建的运行库和 `mksquashfs` 都可完全来自 SDK，不再要求主机安装这些包。

GitHub Actions 的 ARM64 Desktop 发布使用 `build-linux-arm64-cross-appimage.sh`：在 amd64 Ubuntu 18.04 容器中调用 `aarch64-linux-gnu-gcc`、Rust `aarch64-unknown-linux-gnu` target 和 ARM64 多架构 sysroot，编译与 SquashFS 打包均不经过 QEMU。需要启动验证时再单独加入轻量 QEMU smoke test；这样不会把数小时的 Rust 编译放进模拟器。

Linux ARM64 Desktop 的 X11/XCB 运行库默认取自建机 `ldconfig`；当 ARM 主机缺少这些库（典型如 `libxkbcommon-x11.so.0`）时，`build-linux-arm64-appimage.sh` 会按库逐一优先改用 kylin-arm SDK 的 `offline/linux-arm64-ubuntu18/root/usr/lib/aarch64-linux-gnu` 或 `offline/runtime-libs/aarch64-linux-gnu`，其余库仍回退到 ldconfig，也可用 `WUNDER_SLINT_RUNTIME_LIB_DIR=/path/to/libs` 显式指定整个目录。从目录取用的库会校验 AArch64 ELF，防止误指其它架构的库目录。

ARM64 开发 sysroot 首次准备使用 `bash builders/prepare-linux-arm64-devel-sysroot.sh`。如果构建提示 `absolute libgcc_s.so link`、`dangling libgcc_s.so link` 或 `cannot find -lgcc_s`，可在 ARM64 Docker 容器中执行 `bash builders/prepare-linux-arm64-devel-sysroot.sh --repair`；它只修复已有 `Rust-builder/kylin-arm/offline/linux-arm64-ubuntu18/root` 内的软链接，不重新下载 SDK。

## Cargo vendor 树维护

`Rust-builder/win7/offline/cargo-vendor-slint`、`Rust-builder/kylin-arm/offline/cargo-vendor-slint` 与 `Rust-builder/kylin-x86/offline/cargo-vendor-slint` 是锁文件之外的独立快照：`Cargo.toml` 或 `Cargo.lock` 变化后，只改工作区不会让离线构建可用，解析会在 vendor 树里以 `no matching package named ...` 或 `failed to select a version ...` 终止，desktop 与 cli 一起失败。

Cargo 解析的是**完整锁文件闭包**，不是当前目标的编译闭包。即使只构建 Win7 CLI，vendor 里也必须同时存在 macOS/Android 的 `objc2`、`calloop`、`wayland-backend`、wasm 的 `wgpu-core-deps-wasm` 等永远不参与本次编译的 crate，否则解析阶段就会失败。判断是否完整不能只看目录名：vendor 目录既有 `objc2` 这种不带版本的名字，也有 `objc2-0.5.3`，必须读取每个目录 `Cargo.toml` 的 `package.name/version` 后与 `Cargo.lock` 比对。Win7 egui 使用的 `cargo-vendor-egui` 也要单独检查。

补齐的可靠做法是生成一份完整快照再增量复制，而不是猜单个依赖：

```bash
cargo vendor --offline --locked temp_dir/vendor-full
```

该命令从本机已填充的 registry 缓存生成全部锁定包（缺失时联网一次），再按上面的元数据映射把目标 vendor 缺少的 `名字-版本` 目录复制进去。复制后用 `builders/check-vendor-crate.ps1` 逐文件核对校验和，避免半份拷贝静默通过：

```powershell
./builders/check-vendor-crate.ps1 -VendorRoot /path/to/cargo-vendor-slint `
  -Crate vt100-0.15.2, vte-0.11.1, vte_generate_state_changes-0.1.2
```

最后必须验证解析与真实编译两件事：为每棵 vendor 树临时生成只含 `source.crates-io -> vendored-sources` 的 `CARGO_HOME/config.toml`，执行 `cargo metadata --locked --offline` 确认解析；再跑 `build.bat -t desktop -a win7-x86`、`build.bat -t cli -a win7-x86` 或对应的 Linux 构建，确认新 crate 能编过并进入 PE/ELF 门禁。只做 `-Check` 会跳过 codegen 与链接，不能作为最终验收。

2026-10-20 按此方法补齐：win7-slint 增加 9 个（`objc2 0.5.3/0.6.5`、`objc`、`calloop` 等），kylin-arm 与 kylin-x86 各增加 42 个（egui/wgpu 闭包等）；`cargo-vendor-egui` 经元数据审计原本完整。

## Linux amd64 离线 SDK 维护

`kylin-x86` 表示 Linux amd64 宿主。联网准备一次：

```bash
bash builders/build-kylin-x86-sdk-docker.sh
```

生成目录包含固定 Rust 工具链、Cargo vendor、两架构 sysroot tar、宿主工具 tar 和 Docker 镜像归档。Linux 符号链接与权限保存在 tar 中，不能把 Windows 解压结果当成同等 SDK。

离线机器先执行 `docker load -i offline/archives/builder-amd64-ubuntu18.docker.tar`，再用 `--network none` 运行镜像。ARM64 sysroot 从 `offline/archives/linux-arm64-ubuntu18.tar` 解压到 Linux 文件系统；编译器使用镜像的 `aarch64-linux-gnu-gcc`，不能运行 ARM 宿主 sysroot 中的 GCC。Win7 使用 nightly 的 `-Z build-std=std,panic_abort` 与 `i686-w64-mingw32-gcc`，不能用普通 Windows target 替代 Win7 验收。

`metadata/smoke-test.txt` 保存实际禁网工具链验证，`metadata/packages.tsv` 保存系统工具版本，`manifest.json` / `SHA256SUMS` 保存最终文件校验信息。小程序验证不替代完整桌面应用与真实系统运行验收。
