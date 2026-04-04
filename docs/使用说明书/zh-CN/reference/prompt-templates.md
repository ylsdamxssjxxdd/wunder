---
title: 提示词模板参考
summary: Wunder 将系统提示词拆成模板包和分段文件。管理员负责系统模板包，用户侧现在默认提供中英两套内置模板包，并支持按需切换或复制为自定义包。
read_when:
  - 你要修改系统提示词模板包
  - 你要理解用户侧 `default-zh`、`default-en` 与自定义包的关系
  - 你在排查“为什么切换模板包后新线程生效而旧线程不变”
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
  - src/api/admin_prompt_templates.rs
  - src/api/user_prompt_templates.rs
---

# 提示词模板参考

这一页只说明模板包如何组织、如何回退、什么时候生效，不重复解释“为什么需要提示词”。

## 当前分段

系统提示词模板按这些分段组织：

- `role`
- `engineering`
- `tools_protocol`
- `skills_protocol`
- `memory`
- `extra`

你管理的是这些分段文件，不是一整个硬编码的大 prompt。

## 管理员侧模板包

接口：

- `GET /wunder/admin/prompt_templates`
- `POST /wunder/admin/prompt_templates/active`
- `GET/PUT /wunder/admin/prompt_templates/file`
- `POST /wunder/admin/prompt_templates/packs`
- `DELETE /wunder/admin/prompt_templates/packs/{pack_id}`

约束：

- `default` 包只读
- 非 `default` 包默认落在 `./config/data/prompt_templates`
- 管理员切换 active 包后，新的系统模板读取会优先走当前 active 包，缺失分段再回退到系统 `default`

## 用户侧模板包

接口：

- `GET /wunder/prompt_templates`
- `POST /wunder/prompt_templates/active`
- `GET/PUT /wunder/prompt_templates/file`
- `POST /wunder/prompt_templates/packs`
- `DELETE /wunder/prompt_templates/packs/{pack_id}`

用户侧当前默认提供两套只读内置模板包：

- `default-zh`：固定读取中文系统模板
- `default-en`：固定读取英文系统模板

说明：

- 首次使用或历史设置仍为兼容别名 `default` 时，系统会按当前系统语言自动落到 `default-zh` 或 `default-en`
- 用户界面默认展示的是 `default-zh` 和 `default-en`，不再把 `default` 作为主要选项
- 用户手动切换到 `default-zh` 或 `default-en` 后，会固定使用该语言模板，不再跟随系统语言漂移
- 这两个内置包都是只读的，本质上同步当前管理员启用的系统模板包，只是语言锁定不同
- 用户自定义包只影响当前用户后续的新线程

## 用户侧回退链路

可以按下面的顺序理解：

1. 先找当前用户选中的包
2. 如果是自定义包，优先读自定义包对应语言分段
3. 自定义包缺失分段时，回退到当前管理员 active 系统模板包
4. 系统模板包也缺失时，再回退到系统 `default`

对内置包来说：

- `default-zh` 只会读中文模板链路
- `default-en` 只会读英文模板链路

所以用户即使切换了界面语言，只要主动选中了中文包或英文包，运行时 system prompt 也会继续使用该包绑定的语言版本。

## 新建自定义包

用户新建模板包时，通常是“复制当前选中的模板包”：

- 如果当前选中的是 `default-zh` 或 `default-en`，会从当前系统模板内容复制出一份可编辑包
- 如果当前选中的是某个自定义包，则从该自定义包继续复制

这样用户可以先选中中文或英文内置包，再快速派生自己的风格包。

## 生效时机

模板更新后的生效规律很简单：

- 新线程会按最新模板重新构建 system prompt
- 已经初始化并冻结 system prompt 的旧线程不会被回写

这和线程初始化时冻结系统提示词的设计是一致的，不是异常。

## 实施建议

- 想改内置默认内容时，先从 `default-zh` 或 `default-en` 复制出自定义包再改
- 想固定中文或英文风格时，直接切换到对应内置包
- 想让用户端默认风格整体变化时，优先由管理员切换系统 active 模板包

## 延伸阅读

- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [配置说明](/docs/zh-CN/reference/config/)
