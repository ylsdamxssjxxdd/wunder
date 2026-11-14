---
title: 安卓版运行环境
nav_title: 运行环境
summary: 安卓版手机里到底装了什么：APK 自带的 Ubuntu rootfs 与工具清单、两套浏览器内核、权限要求，以及 proot 的能力边界。
updated_at: 2026-11-14
---

# 安卓版运行环境

安卓版把桌面版的引擎与一套 Ubuntu 环境一起塞进 APK：装上就能在手机上跑命令、读写文件、调用工具，不需要 root，也不需要联网下载运行环境。

## 组成

| 部分 | 说明 |
| --- | --- |
| 系统要求 | Android 8.0（API 26）起，arm64-v8a；x86_64 切片只用于模拟器验证 |
| 界面 | 单列手机工作台，复用桌面版的 Slint 界面代码 |
| 命令环境 | 内置 Ubuntu arm64 根文件系统，由 proot 在应用私有目录里运行，不申请 root |
| 数据 | 全部在应用私有目录，卸载即清除 |

## 内置工具

根文件系统里预装常用工具，智能体直接可用：

- **开发**：python、git、ripgrep、7zip、静态 ffmpeg、binutils、patchelf、upx、sqlite3
- **系统**：curl、wget、zip/unzip、procps、psmisc、lsof、bc、bzip2、time、iproute2、rsync、tmux
- **字体**：fontconfig 加 DejaVu、Liberation、Noto Color Emoji、文泉驿微米黑，覆盖拉丁、等宽、中文与 emoji
- **手机镜像与恢复工具箱**：adb、simg2img、erofs-utils、debugfs、squashfs-tools、mtools/dosfstools/exfatprogs/ntfs-3g、testdisk/photorec、ddrescue、binwalk、foremost、nmap

这些工具只做文件级操作：proot 没有块设备、FUSE 和 raw socket，`fastboot`、`mount`、`tcpdump`、USB 与蓝牙控制这类依赖内核权限的操作不可用。

## 两套浏览器内核

| 场景 | 用什么 | 前提 |
| --- | --- | --- |
| 普通网页操作 | 手机系统 WebView，由悬浮窗承载 | 首次调用需授予「显示在其他应用上层」权限 |
| 逐帧渲染类技能（如知识短片） | Playwright 1.40 与 Chromium 1091，装在 `/opt/browser/chrome-linux` | 随发布 APK 一起带上，开箱可用 |

开发构建可以不带浏览器包，此时 WebView 仍然可用，只是渲染类技能会提示缺少运行时。

## 能力边界

- 没有端口、没有常驻服务，也不提供对外访问入口。
- 与桌面版共享同一套工具语义，但系统级能力（托盘、全局热键、桌面控制）在手机上不存在。
- 交互式终端的键位按手机输入方式调整，见[安卓版使用](/docs/zh-CN/forms/android-usage/)。
