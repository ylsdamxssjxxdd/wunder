---
title: 节点调用
summary: `节点调用` 是 Wunder 的底层节点桥接工具，既能列出可用节点，也能把具体命令和参数发到指定节点执行。
read_when:
  - 你要理解节点桥接能力
  - 你要区分节点调用和浏览器/桌面控制的边界
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# 节点调用

`节点调用` 是更底层的系统桥接工具。

如果说浏览器和桌面控制是面向具体交互动作，那么节点调用更像“把命令发给指定节点”。

## 核心动作

- `list`
- `invoke`

同时它也兼容一种简写方式：

- 直接给 `node_id + command`

## 常用参数

- `action`
- `node_id`
- `command`
- `args`
- `timeout_s`
- `metadata`

## 它适合什么

- 列出系统当前可用节点
- 把命令发送给某个指定节点
- 做更底层的节点级桥接调用

## 和浏览器、桌面控制的区别

- [浏览器](/docs/zh-CN/tools/browser/) 面向网页交互。
- [桌面控制](/docs/zh-CN/tools/desktop-control/) 面向桌面动作。
- `节点调用` 面向底层节点命令。

如果你已经有更明确的浏览器或桌面动作，不必先走节点调用。

## 你最需要记住的点

- `节点调用` 更底层，也更通用。
- 最常见动作是 `list` 和 `invoke`。
- 它更像系统桥接层，不是普通用户第一优先工具。

## 相关文档

- [浏览器](/docs/zh-CN/tools/browser/)
- [桌面控制](/docs/zh-CN/tools/desktop-control/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
