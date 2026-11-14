---
title: 命令行部署
nav_title: 部署
summary: 命令行部署：系统要求、获取方式、启动参数与运行时文件位置。
updated_at: 2026-11-14
---

# 命令行部署

## 部署

- **系统**：Windows 7 起（GNU 工具链）；Linux x64 / arm64（ubuntu 18.04 sysroot）。
- **获取**：随桌面版安装包一起装好；也可用 `cargo build --release -p wunder-cli` 自行构建。
- **启动**：无必需参数，直接运行 `wunder-cli`；工作区默认是启动命令时所在的目录，`-C/--cd` 可指定其他目录。
- **数据**：`WUNDER_HOME` → `~/.wunder`；运行时配置与 SQLite 状态在 `~/.wunder/cli/WUNDER_TEMP/`，用户配置 `~/.wunder/config.toml`，引擎配置与桌面版共用同一份。
- **运行形态**：stdin / stdout / stderr 全是 TTY 时进交互界面；否则自动降级为一次性执行，管道与脚本走这条。
- `wunder-cli doctor` 检查本地运行环境是否就绪。
