# 离线部署验证记录

日期：2026-10-27。准备机为 Windows 上的 Docker Linux，ARM64 容器通过已有仿真运行；不代表已在目标服务器验收。

## 架构

- Gitea、Runner、构建系统环境三个镜像单独导出。
- Rust-builder 作为独立的 `rust-builder/offline-sdk.tar.gz` 交付，不复制到构建镜像。
- 宿主机解压后，Runner 和任务容器通过同一个绑定命名卷只读挂载 SDK；编译缓存独立持久化。
- 默认工作流仅覆盖 Linux arm64/amd64 的蛹 AppImage、舵机 ELF；Win7/WASM 工具虽保留在 SDK 中，但未纳入默认发布矩阵。

## 验证范围

挂载版已通过以下验证：

- 构建镜像不含 Rust-builder；未挂载时没有 Rust 工具链，挂载后可直接运行。
- 独立 SDK 归档的 SHA256、gzip/tar 布局、符号链接和执行权限；三个镜像归档的校验与构建镜像 docker load 回载。
- `prepare-sdk.sh` 完整离线包解压、重复执行、自定义目录及空格路径、记住已有路径、拒绝相对路径；损坏或不完整归档不会留下已安装的半成品目录。
- Compose 绑定命名卷与 Runner 任务容器只读挂载：任务可执行 SDK 工具，但不能写入 SDK，且不能访问 Docker socket、初始化凭证和外网。
- 真实私有仓库 push 自动触发两个架构的小型 Rust 项目：ARM64 原生编译、AMD64 交叉编译，调用项目原有四套 AppImage/CLI 构建脚本，ELF 架构与 glibc 上限校验通过；短期工作流令牌将两个架构附件和校验清单上传至同一 Release。
- ARM64 小型 CLI 与从 AppImage 解包后的 AppRun 实际运行成功。准备机 QEMU 的 binfmt 匹配不识别 AppImage 特有 ELF 标记，因此未直接启动 AppImage runtime；没有把仿真环境的 Exec format error 当成目标 ARM 服务器实测结论。
- Gitea 首次初始化、Runner 自动注册，以及重复 Compose 启动保持已有数据。

本项目两个架构的完整 Release 编译尚未完成；此前启动的冻结源码验证已停止，缓存保留，不改动工作区。不能将小项目流水线成功解释为本项目全量产物或所有保留目标已经验证通过。离线 SDK 更新必须与提交中的 Cargo.lock 匹配。
