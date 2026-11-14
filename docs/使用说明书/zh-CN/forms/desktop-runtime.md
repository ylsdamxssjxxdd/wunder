---
title: 桌面版运行环境
nav_title: 运行环境
summary: 桌面版跑起来靠什么：单进程内置引擎、安装根下的 opt 预置组件与版本、系统依赖、数据库和端口情况。
updated_at: 2026-11-14
---

# 桌面版运行环境

桌面版把引擎装进同一个进程：界面、智能体调度、工具执行都在本机跑，不经本机 HTTP/WebSocket 中转，就不需要另外起服务。它需要的运行环境只有两块——程序本体，和安装根下的预置组件。

## 组成

| 部分 | 在哪 | 作用 |
| --- | --- | --- |
| 程序本体 | `<安装根>/app/desktop` | 界面（Slint）与引擎，自带所需运行库 |
| 启动器 | `<安装根>/bin/wunder-desktop` | 导出 `WUNDER_HOME` 后拉起本体 |
| 预置组件 | `<安装根>/opt/` | Python、Git、ripgrep、FFmpeg、7-Zip、浏览器、字体 |
| 本地状态 | `<安装根>/cli/WUNDER_TEMP/` | 会话、工具日志与 SQLite |

## 预置组件

桌面版与命令行共用同一份 `opt/`，装桌面版就等于装好了命令行的运行环境。

| 组件 | 版本 | 给谁用 |
| --- | --- | --- |
| `opt/python` | Windows 3.8.10；Linux 3.8.18 | 智能体写脚本、跑 Python 工具 |
| `opt/git` | MinGit 2.46.2（Windows）/ 2.46.2 源码编译（Linux） | clone、补丁、仓库操作 |
| `opt/rg` | Windows 14.1.0；Linux 14.1.1 | 搜索内容与命令行检索 |
| `opt/ffmpeg` | 静态 4.4.1 | 抽帧、抽音轨、转码（不含 `ffprobe`） |
| `opt/7zip` | 25.01 | 压缩包打包与解压 |
| `opt/browser` | Windows 内置 Chrome 109（win32）；Linux amd64 为 Chrome for Testing 122，arm64 为 Chromium 120 | 浏览器工具的离线内核 |
| `opt/fonts` | 公文常用字体 + 短片字体 | 文档排版、绘图与短片渲染 |
| `opt/powershell` | PowerShell 7（仅 Windows） | 命令执行 |

组件版本跟随补充包走，不随程序本体升级；换机器时整个安装根一起复制即可，`opt/` 会跟过去。

## 系统依赖

- Windows 7 x86 起；Linux 以 ubuntu 18.04 为基线（glibc 2.27），更老的 glibc 不行，更新的发行版可用。
- 界面与运行库都打进 `app/desktop`，系统不需要另装 GTK、Python 或浏览器。
- 安装 `.run` 时需要系统有 `xz`（Ubuntu 18.04 及更新版本默认自带；最小化系统补 `xz-utils`）。装好之后运行不再依赖它。

## 数据库、端口与网络

- 状态存在本地 SQLite，没有外部数据库，也不需要单独备份数据库服务。
- 默认不监听任何端口，没有可供外部访问的管理地址；需要跨机能力时走互通隧道或云端工作区。
- 连模型、抓网页、访问网盘这类动作由一次性的出站请求完成，不常驻端口。

## 装好之后先自检

1. 设置里能打开「工具管理」，说明组件目录被识别到。
2. 对话里让它跑一句 `python -V` 或 `git --version`，版本对得上上表就是通的。
3. 跑不起来时先确认进程是否用了预期的安装根——设置了非空的 `WUNDER_HOME` 时以它为准，会绕过安装根标记。

命令行的 `wunder-cli doctor` 能做更细的检查，见[命令行运行环境](/docs/zh-CN/forms/cli-runtime/)。
