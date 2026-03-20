---
title: 提示词模板参考
summary: Wunder 把系统提示词拆成模板包和分段文件，并同时提供管理员侧与用户侧的模板管理接口。
read_when:
  - 你要修改系统提示词模板包
  - 你想知道 default 包、用户包和线程冻结之间的关系
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
  - src/api/admin_prompt_templates.rs
  - src/api/user_prompt_templates.rs
---

# 提示词模板参考

这页是模板治理的参考页，不再重复解释“为什么需要提示词”。

## 本页重点

这页只回答这些问题：

- 模板包有哪些分段
- 管理员和普通用户分别走哪些接口
- 改完模板后什么时候会生效

## 当前分段键

系统提示词模板当前按这些分段组织：

- `role`
- `engineering`
- `tools_protocol`
- `skills_protocol`
- `memory`
- `extra`

你改的不是一整块大 prompt，而是这些分段文件。

## 管理员侧模板接口

- `GET /wunder/admin/prompt_templates`
- `POST /wunder/admin/prompt_templates/active`
- `GET/PUT /wunder/admin/prompt_templates/file`
- `POST /wunder/admin/prompt_templates/packs`
- `DELETE /wunder/admin/prompt_templates/packs/{pack_id}`

重点约束：

- `default` 包只读
- 非 default 包默认落在 `./data/prompt_templates`

## 用户侧模板接口

- `GET /wunder/prompt_templates`
- `POST /wunder/prompt_templates/active`
- `GET/PUT /wunder/prompt_templates/file`
- `POST /wunder/prompt_templates/packs`
- `DELETE /wunder/prompt_templates/packs/{pack_id}`

重点约束：

- 用户自己的 `default` 包会跟随系统当前 active 模板同步
- 用户自建包只影响自己的新线程

## 生效时机

这是最容易误解的一点。

结论很简单：

- 改模板后，优先影响新线程
- 已经冻结的旧线程不会被回写

所以模板治理和线程稳定性是一起设计的，不是矛盾关系。

## 回退链路怎么理解

当前回退语义可以简单记成：

- 先找当前 active 包
- 缺失分段时回退系统 active
- 再缺失时回退系统 default

所以你不必每次都复制整套模板，按需覆盖分段即可。

## 什么时候最需要看这页

- 管理端切换 prompt template pack
- 用户想维护自己的提示词风格包
- 你在解释“为什么改完模板当前线程没变”

## 实施建议

- `default` 包只读，建议先复制再改。
- 模板是分段治理，不是单文件硬编码。
- 模板更新影响新线程，不追写旧线程。

## 延伸阅读

- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [配置说明](/docs/zh-CN/reference/config/)
