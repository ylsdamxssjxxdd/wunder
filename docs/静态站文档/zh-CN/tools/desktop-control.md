---
title: 桌面控制
summary: `桌面控制器` 和 `桌面监视器` 让 Wunder 能在 desktop 场景下基于坐标框执行桌面动作和截图观察。
read_when:
  - 你要理解 bbox 驱动的桌面操作
  - 你在看 desktop 自动化链路
source_docs:
  - docs/API文档.md
  - src/services/tools/desktop_control.rs
  - docs/设计方案.md
---

# 桌面控制

Wunder 的桌面自动化主要由两个工具组成：

- `桌面控制器`
- `桌面监视器`

## `桌面控制器` 解决什么

它基于 `bbox + action` 执行桌面操作。

常见动作包括：

- `left_click`
- `left_double_click`
- `right_click`
- `scroll_down`
- `scroll_up`
- `press_key`
- `type_text`
- `move_mouse`
- `drag_drop`
- `delay`

常用参数：

- `bbox`
- `action`
- `description`
- `key`
- `text`
- `delay_ms`
- `duration_ms`
- `scroll_steps`
- `to_bbox`

## 为什么一定要有 `description`

因为桌面动作比文件工具更危险，执行时需要把目标动作说明清楚，便于日志、审计和后续排障。

## `桌面监视器` 解决什么

它更偏观察。

最常见参数是：

- `wait_ms`
- `note`

它会等待一段时间后返回桌面截图，用来观察界面有没有变化。

## 两者怎么配合

常见路径是：

1. 先监视或截图确认当前状态
2. 再用控制器点击、输入或拖拽
3. 再次监视确认结果

## 实施建议

- 桌面控制器负责动作，桌面监视器负责观察。
- `bbox` 是桌面操作的核心定位信息。
- 这组工具属于 desktop 场景，不应按普通 server 内置工具理解。

## 延伸阅读

- [浏览器](/docs/zh-CN/tools/browser/)
- [Desktop 界面](/docs/zh-CN/surfaces/desktop-ui/)
