---
title: 临时目录与文档转换
summary: `/wunder/temp_dir/*` 负责临时上传、下载和中转；文档转换与聊天附件预处理分别走公共转换、调试转换和聊天域转换接口。
read_when:
  - 你要给外部系统发下载链接
  - 你要区分工作区、temp_dir 和 doc2md 的职责
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/api/temp_dir.rs
  - src/api/doc2md.rs
  - src/api/chat.rs
---

# 临时目录与文档转换

在 Wunder 里，`temp_dir` 是中转层，不是正式工作区。

## 本页重点

这页只解释四件事：

- 什么应该放进 `temp_dir`
- 什么情况下应该先经过文档转换
- 聊天域的附件预处理接口怎么分
- 为什么很多外部渠道最终拿到的是 `/wunder/temp_dir/download`

## 最常用的接口

- `POST /wunder/doc2md/convert`
- `POST /wunder/attachments/convert`
- `POST /wunder/chat/attachments/convert`
- `POST /wunder/chat/attachments/media/process`
- `GET /wunder/temp_dir/download`
- `POST /wunder/temp_dir/upload`
- `GET /wunder/temp_dir/list`
- `POST /wunder/temp_dir/remove`

## 什么时候应该用它

- 你要临时上传一个文件给系统处理
- 你要给外部客户端发一个可点击下载链接
- 你要先把 doc/pdf/ppt/xlsx 之类文件转成 Markdown
- 你在做调试面板附件解析
- 你在做聊天输入区的文档、音频或视频附件预处理

## 四类高频接口怎么分

可以这样记：

| 接口 | 面向对象 | 典型输出 |
|------|----------|----------|
| `/wunder/doc2md/convert` | 公共文档转换 | Markdown 内容 |
| `/wunder/attachments/convert` | 调试面板 / 鉴权联调 | 与 `doc2md` 一致，但要求鉴权 |
| `/wunder/chat/attachments/convert` | 聊天输入区的文档附件 | 供聊天域装配文本型 `attachments` |
| `/wunder/chat/attachments/media/process` | 聊天输入区的音频 / 视频附件 | 音频转写结果，或视频拆出的图片帧 + 音轨附件 |

补充两点：

- 图片一般不走这些转换接口，直接作为聊天附件发送即可。
- 视频不会直接发给模型，而是先拆成图片序列和音轨；重新抽帧依赖 `source_public_path`。

## 为什么很多文件最后变成 `temp_dir` 下载链接

因为很多外部客户端并不理解 Wunder 内部的工作区路径。

所以系统会把：

- `/workspaces/...`

改写成：

- `/wunder/temp_dir/download?...`

这样渠道客户端或外部网页才能真正点开。

聊天媒体预处理时，源文件通常先落到工作区公共路径；真正发给外部客户端下载时，系统仍可能再改写成 `temp_dir` 下载链接。

## 常见误区

### 把 `temp_dir` 当长期存储

不对。

它是中转区，不是长期业务资料区。

### 把转换后的 Markdown 直接当工作区主文件

不一定。

先判断你需要的是“临时消费”还是“后续持续处理”。

### 以为 `temp_dir` 只给管理端用

也不对。

它是正式公共中转层，很多外部渠道链路都会用到。

## 实施建议

- `temp_dir` 适合中转和分发，不适合长期沉淀业务文件。
- 文档类转换先分清是公共能力、调试面板还是聊天输入区。
- 音频 / 视频附件应走 `POST /wunder/chat/attachments/media/process`，不要直接把原始视频当模型输入。
- 外部渠道能点击打开文件，通常依赖的是 `temp_dir` 下载链接。

## 延伸阅读

- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [数据与存储](/docs/zh-CN/ops/data-and-storage/)
