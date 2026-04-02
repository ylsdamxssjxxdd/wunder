---
title: LSP 查询
summary: LSP查询 面向代码语义导航，提供定义跳转、引用搜索、悬停说明、符号树和调用层级，与文本搜索分工明确。
read_when:
  - 你要查定义、引用或调用关系
  - 你要区分 LSP 查询和文本搜索的边界
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# LSP 查询

代码语义导航工具，基于 LSP 协议。

---

## 功能说明

`LSP查询` 面向代码语义导航，提供定义跳转、引用搜索、悬停说明、符号树和调用层级。

**别名**：
- `lsp`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `operation` | string | ✅ | 要执行的操作 |
| `path` | string | ❌ | 文件路径 |
| `line` | integer | ❌ | 行号（1-based） |
| `character` | integer | ❌ | 列号（1-based） |
| `query` | string | ❌ | 查询字符串 |
| `call_hierarchy_direction` | string | ❌ | 调用层级方向：`incoming` 或 `outgoing` |

---

## 支持的操作

| 操作 | 说明 | 必填参数 |
|------|------|----------|
| `definition` | 查找定义 | `path`, `line`, `character` |
| `references` | 查找引用 | `path`, `line`, `character` |
| `hover` | 悬停说明 | `path`, `line`, `character` |
| `documentSymbol` | 文档符号 | `path` |
| `workspaceSymbol` | 工作区符号 | `query` |
| `implementation` | 查找实现 | `path`, `line`, `character` |
| `callHierarchy` | 调用层级 | `path`, `line`, `character`, `call_hierarchy_direction` |

---

## 使用示例

### 查找定义

```json
{
  "operation": "definition",
  "path": "src/main.rs",
  "line": 10,
  "character": 5
}
```

### 查找引用

```json
{
  "operation": "references",
  "path": "src/main.rs",
  "line": 10,
  "character": 5
}
```

### 查找符号

```json
{
  "operation": "workspaceSymbol",
  "query": "execute_tool"
}
```

### 查看文件结构

```json
{
  "operation": "documentSymbol",
  "path": "src/main.rs"
}
```

### 查看调用层级

```json
{
  "operation": "callHierarchy",
  "path": "src/main.rs",
  "line": 10,
  "character": 5,
  "call_hierarchy_direction": "outgoing"
}
```

---

## 处理流程

1. 校验 `lsp.enabled` 配置
2. 校验目标文件存在
3. 让对应文件进入 LSP 管理器的观察范围
4. 把请求分发给当前命中的 LSP 客户端

---

## 与搜索内容的对比

| 特性 | LSP查询 | [搜索内容](/docs/zh-CN/tools/workspace-files/) |
|------|---------|--------------------------------|
| 目标 | 代码语义导航 | 模糊文本搜索 |
| 输入 | 位置或符号名 | 关键词 |
| 推荐使用 | 找定义、引用、调用链 | 找关键词 |

---

## 适用场景

✅ **适合使用 LSP查询**：
- 想找某个函数在哪定义 → 用 `definition`
- 想找所有调用点 → 用 `references`
- 想看文件结构 → 用 `documentSymbol`
- 想找工作区符号 → 用 `workspaceSymbol`
- 想看调用层级 → 用 `callHierarchy`

❌ **不适合使用 LSP查询**：
- 只有关键词，没有明确代码位置 → 先用文本搜索

---

## 注意事项

1. **1-based 位置**：
   - `line` 和 `character` 都是从 1 开始计数
   - 不是 0-based

2. **多个 LSP 服务端**：
   - 返回结果可能来自多个 LSP 服务端
   - 不一定只有一份结果

3. **不是全文检索**：
   - 如果只有关键词，没有明确代码位置
   - 先用文本搜索通常更快

---

## 延伸阅读

- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [系统架构](/docs/zh-CN/concepts/architecture/)
