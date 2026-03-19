---
title: 临时目录与文档转换
summary: `/wunder/temp_dir/*` 负责临时上传、下载和中转，`/wunder/doc2md/convert` 负责把文档转换成更适合模型消费的 Markdown。
read_when:
  - 你要给外部系统发下载链接
  - 你要区分工作区、temp_dir 和 doc2md 的职责
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - src/api/temp_dir.rs
  - src/api/doc2md.rs
---

# 临时目录与文档转换

在 Wunder 里，`temp_dir` 是中转层，不是正式工作区。

## 这页解决什么

这页只解释三件事：

- 什么应该放进 `temp_dir`
- 什么情况下应该先经过 `doc2md`
- 为什么很多外部渠道最终拿到的是 `/wunder/temp_dir/download`

## 最常用的接口

- `POST /wunder/doc2md/convert`
- `POST /wunder/attachments/convert`
- `GET /wunder/temp_dir/download`
- `POST /wunder/temp_dir/upload`
- `GET /wunder/temp_dir/list`
- `POST /wunder/temp_dir/remove`

## 什么时候应该用它

- 你要临时上传一个文件给系统处理
- 你要给外部客户端发一个可点击下载链接
- 你要先把 doc/pdf/ppt/xlsx 之类文件转成 Markdown
- 你在做调试面板附件解析

## `doc2md` 和 `attachments/convert` 的区别

可以这样记：

- `/wunder/doc2md/convert`：公共转换入口，无需鉴权
- `/wunder/attachments/convert`：调试面板使用，逻辑与 `doc2md` 一致，但需鉴权

所以如果你只是做文档转换能力接入，优先看 `doc2md`。

## 为什么很多文件最后变成 `temp_dir` 下载链接

因为很多外部客户端并不理解 Wunder 内部的工作区路径。

所以系统会把：

- `/workspaces/...`

改写成：

- `/wunder/temp_dir/download?...`

这样渠道客户端或外部网页才能真正点开。

## 最容易犯的错

### 把 `temp_dir` 当长期存储

不对。

它是中转区，不是长期业务资料区。

### 把转换后的 Markdown 直接当工作区主文件

不一定。

先判断你需要的是“临时消费”还是“后续持续处理”。

### 以为 `temp_dir` 只给管理端用

也不对。

它是正式公共中转层，很多外部渠道链路都会用到。

## 你最需要记住的点

- `temp_dir` 适合中转和分发，不适合长期沉淀业务文件。
- `doc2md` 负责把多种文档变成模型更容易消费的 Markdown。
- 外部渠道能点击打开文件，通常依赖的是 `temp_dir` 下载链接。

## 相关文档

- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [数据与存储](/docs/zh-CN/ops/data-and-storage/)
