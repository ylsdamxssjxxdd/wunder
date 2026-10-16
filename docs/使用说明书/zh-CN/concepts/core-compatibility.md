---
title: 兼容性
summary: 蛹与舵机在 Windows 7 32 位与 Ubuntu 18.04 上的离线构建链、软件渲染与终端适配。
updated_at: 2026-10-16
---

# 兼容性

## 系统支持

| 系统 | 做法 |
| --- | --- |
| Windows 7 32 位 | 专用 `i686-win7-windows-gnu` Tier 3 目标 + build-std 离线工具链 |
| Ubuntu 18.04 x64 / arm64 | 离线构建链与 sysroot |

## 运行环境约束

- **界面不依赖 GPU**：Slint 走软件渲染。
- **构建不依赖联网**：Windows 7 32 位与 Ubuntu 18.04 两条链都是离线工具链，Ubuntu 侧另配 sysroot。
- **命令会话适配旧终端**：命令会话用 winpty 承载，输出按 GBK 解码。
- **启动检查**：启动时做恢复扫描与自检。

两条链离线可用，界面不依赖 GPU，命令会话适配旧终端——Windows 7 32 位与 Ubuntu 18.04（x64 / arm64）上可以直接安装运行蛹与舵机。

## 限制

- 构建脚本仍残留 Tauri 死链——指向已不存在的目录，属工程缺口，不代表能力。
- win7 互通节点不声明 `tool.exec`，旧设备的能力面因此收窄。
