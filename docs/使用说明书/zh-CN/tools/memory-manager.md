---
title: 记忆管理
summary: 说明 `memory_manager` 的当前动作、精简结构，以及“短索引注入 + get 取全文”的使用方式。
read_when:
  - 你要新增、检索、读取、更新或删除长期记忆碎片
source_docs:
  - src/services/tools/memory_manager_tool.rs
  - src/services/memory_fragments.rs
updated_at: 2026-04-12
---

# 记忆管理

`memory_manager` 用于管理当前智能体在当前用户作用域下的长期记忆。

当前标准动作：

- `list`
- `search`
- `get`
- `add`
- `update`
- `remove`
- `clear`

## 使用原则

- system prompt 默认只注入短 `memory_id + title` 索引
- 不会自动注入完整 `content`
- 需要完整细节时，先用 `list/search` 找到 `memory_id`，再用 `get`
- 模型侧 `memory_id` 尽量保持 8 个字符以内，方便搜索与定位
- `add/update` 只使用当前核心字段：
  - `title`
  - `content`
  - `tag`
  - `related_memory_id`
  - `memory_time`

## `list`

返回最近记忆索引，默认 30 条。

```json
{
  "action": "list",
  "data": {
    "count": 1,
    "items": [
      {
        "memory_id": "0695f345",
        "title": "用户姓名",
        "tag": "profile",
        "updated_at": 1775957851
      }
    ]
  }
}
```

## `search`

搜索标题或内容匹配项，默认 10 条。

```json
{
  "action": "search",
  "data": {
    "query": "周华健",
    "count": 1,
    "items": [
      {
        "memory_id": "0695f345",
        "title": "用户姓名",
        "tag": "profile",
        "snippet": "用户的名字是周华健。这是用户在对话开始时自我介绍提供的信息。",
        "matched_in": ["content"],
        "updated_at": 1775957851
      }
    ]
  }
}
```

## `get`

按 `memory_id` 读取完整内容。

```json
{
  "action": "get",
  "data": {
    "memory_id": "0695f345",
    "item": {
      "memory_id": "0695f345",
      "title": "用户姓名",
      "content": "用户的名字是周华健。这是用户在对话开始时自我介绍提供的信息。",
      "tag": "profile",
      "related_memory_id": null,
      "memory_time": 1775957820,
      "updated_at": 1775957851
    }
  }
}
```

## 写入示例

```json
{
  "action": "add",
  "title": "用户姓名",
  "content": "用户的名字是周华健。这是用户在对话开始时自我介绍提供的信息。",
  "tag": "profile",
  "memory_time": "2026-04-12T08:37:00+08:00"
}
```

## 关键约束

以下旧字段不再作为推荐输入：

- `summary`
- `tags`
- `entities`
- `category`

实用规则：

- `list/search` 是主检索动作
- `get` 是唯一完整详情读取动作
- 写入只影响后续新线程，不会回写已冻结线程的 system prompt
