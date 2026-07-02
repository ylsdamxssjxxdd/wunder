---
title: 技能调用
summary: `skill_call` 如何把技能正文、根目录和一级文件树返回给模型。
read_when:
  - 用户要加载某个 `SKILL.md` 及其目录信息
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# 技能调用

`skill_call` 的作用是**把技能内容加载进当前上下文**，让模型按技能流程继续工作。

## 最小参数

```json
{
  "name": "openai-docs"
}
```

## 成功返回

```json
{
  "ok": true,
  "action": "skill_call",
  "state": "completed",
  "summary": "Loaded skill openai-docs.",
  "data": {
    "name": "openai-docs",
    "description": "Use official OpenAI docs...",
    "path": "C:/.../SKILL.md",
    "root": "C:/.../openai-docs",
    "content": "# SKILL ... {{SKILL_ROOT}} ...",
    "tree": [
      "SKILL.md",
      "references/",
      "scripts/"
    ]
  }
}
```

## 重点变化

- `content` 会做适合模型阅读的渲染
- `root` 会明确返回技能根目录
- 文中可用 `{{SKILL_ROOT}}` 占位
- `tree` 只列出技能根目录下一级文件和文件夹，让模型知道可继续读取哪些附加资源

## 常见失败

- 技能名为空
- 技能不存在
- 技能名有歧义
- 技能文件读取失败

这些错误通常直接以普通错误返回，不属于复杂异步工具。
