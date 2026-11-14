---
title: 桌面版部署
nav_title: 部署
summary: 桌面版部署：系统要求、安装包、安装根与本地数据位置。
updated_at: 2026-11-14
---

# 桌面版部署

## 部署

### 系统与安装包

- 系统：Windows 7 x86 起；Linux（ubuntu 18.04 基线）x64 / arm64；现代 Windows / Linux 同样可运行。
- Windows：下载 `wunder-desktop-*-win7-x86-setup.exe`，向导默认全选，本机已有的组件可以取消。
- Linux：下载 `wunder-desktop-*-linux-*.run`，全程没有提问，桌面版、命令行与补充组件（Python / Git / rg / ffmpeg 等）一次装好，并自动创建应用菜单项与桌面图标，不需要 FUSE 或管理员权限。
- 另提供 `.tar.gz` 与 `.portable` 产物。
- 安装根默认 `~/.wunder`，Linux 可用 `--home=PATH` 改到任意可写目录；装好后可执行文件在 `bin/wunder-desktop`，安装器会把 `bin` 加入 PATH（Windows）或在 `~/.local/bin` 建命令链接（Linux）。

### 本地数据

| 内容 | 位置 |
| --- | --- |
| 根目录 | 安装根（`WUNDER_HOME`，默认 `~/.wunder`） |
| 工作区 | `<根目录>/workspace` |
| 临时目录 | `WUNDER_DESKTOP_TEMP_ROOT` |
| 桌面设置 | `<temp>/config/desktop.settings.json` |
| 会话与工具日志 | 本地 SQLite |

迁到新机器：复制整个安装根即可。启动时会扫描工作树（最多 4096 项），恢复上次没跑完的任务状态，并清理会话锁与残留任务。
