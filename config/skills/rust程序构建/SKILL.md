---
name: rust程序构建
description: 构建 Rust CLI、服务和桌面应用的离线交叉编译技能。支持默认 Win7 32 位、Linux amd64/arm64 目标；桌面默认 frontend-slint，并包含 Slint 检查与渲染流程。需要使用 Rust-builder 离线 SDK、Cargo vendor 或交叉编译工具链时使用。
---

# RUST 程序构建

## 目标与约束

- 先识别产物类型：`cli`、`service` 或 `desktop`。桌面默认使用仓库的 `frontend-slint/`，不要切换到 Electron/Tauri。
- 未指定目标时使用 `win7-x86`：`i686-win7-windows-gnu`，面向 Windows 7 32 位。
- 支持 `linux-amd64-ubuntu18` 和 `linux-arm64-ubuntu18`；Kylin x86 统一按 Linux amd64 处理，目标为 `x86_64-unknown-linux-gnu`。
- 构建必须可离线重复：设置 `CARGO_NET_OFFLINE=true`，优先使用已提交的 `Cargo.lock`、vendor 源和固定 toolchain；禁止为了补依赖自动联网。
- 大型 SDK 不复制进技能目录。通过 `--sdk-root` 或 `RUST_BUILDER_ROOT` 指向 Rust-builder；默认探测项目旁的 `Rust-builder`。SDK 缺失时停止并给出导入路径。

## 标准流程

1. 运行 `scripts/preflight.py --target <目标> --sdk-root <路径>`，检查 toolchain、target、linker、sysroot、vendor 和哈希清单。
2. CLI/服务执行 `scripts/build.ps1`（Windows）或 `scripts/build.sh`（Linux/macOS）；传入 `--kind cli|service`、`--target`、`--profile`、`--package`、`--jobs`、`--output`。
3. 桌面构建前读取 [references/slint-build.md](references/slint-build.md)，先用 `slint-viewer --check`，再用 `--screenshot` 或 MCP 查看渲染，最后执行 Rust 构建。
4. 构建后执行 `scripts/package.py`：检查 PE/ELF 架构、Win7 API 或 GLIBC 上限，strip（若可用），生成 SHA-256 清单。不要把“编译成功”当作兼容性验收。
5. 在真实 Win7 或对应 Linux amd64/Ubuntu18 环境做运行验收；记录未能实机验证的限制。

## 目标选择

| ID | 默认产物 | Rust target | SDK profile |
|---|---|---|---|
| `win7-x86` | CLI/服务/Slint EXE | `i686-win7-windows-gnu` | `win7` |
| `linux-amd64-ubuntu18` | CLI/服务/Slint | `x86_64-unknown-linux-gnu` | Linux amd64 sysroot |
| `linux-arm64-ubuntu18` | CLI/服务/Slint | `aarch64-unknown-linux-gnu` | `kylin-arm` |

详细目录契约、交叉编译变量和兼容性门禁见 [references/targets.md](references/targets.md) 与 [references/offline-sdk.md](references/offline-sdk.md)。

## Linux amd64 SDK

通过 `builders/build-kylin-x86-sdk-docker.sh` 在 Linux amd64 Ubuntu18 Docker 容器中准备 Rust-builder 的 amd64 离线 SDK。构建技能只使用 `linux-amd64-ubuntu18` 目标；生成目录可按 Rust-builder 约定命名为 `kylin-x86`。

## 常见失败

- vendor 缺失：先执行项目既有 `cargo vendor`（在联网准备机），再把 vendor 和 Cargo index 一并归档；离线机不执行 `cargo fetch`。
- linker 架构错误：检查 host/target 的 linker wrapper，禁止全局设置 `GCC_EXEC_PREFIX`。
- Win7 启动失败：检查 PE 为 i386、MinGW 运行库和 Windows 7 API 黑名单；桌面程序还需在真实 Win7 验收。
- Linux 启动失败：用 `readelf` 检查机器架构和 `GLIBC_` 版本不得高于 2.27。

