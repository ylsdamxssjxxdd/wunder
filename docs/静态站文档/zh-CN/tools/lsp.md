---
title: LSP 查询
summary: `LSP查询` 把定义跳转、引用搜索、悬停说明、符号树和调用层级做成正式工具，适合代码导航而不是全文模糊搜索。
read_when:
  - 你要查定义、引用或调用关系
  - 你要区分 LSP 查询和文本搜索的边界
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# LSP 查询

`LSP查询` 面向“代码语义导航”，不是面向普通字符串搜索。

## 核心操作

- `definition`
- `references`
- `hover`
- `documentSymbol`
- `workspaceSymbol`
- `implementation`
- `callHierarchy`

## 常用参数

- `operation`
- `path`
- `line`
- `character`
- `query`
- `call_hierarchy_direction`

其中：

- `definition`、`references`、`hover`、`implementation`、`callHierarchy` 需要 `line` 和 `character`。
- `workspaceSymbol` 需要 `query`。
- `call_hierarchy_direction` 支持 `incoming` 和 `outgoing`。

## 它怎么工作

在执行查询前，系统会先：

- 校验 `lsp.enabled`
- 校验目标文件存在
- 让对应文件进入 LSP 管理器的观察范围

随后再把请求分发给当前命中的 LSP 客户端，所以返回结果可能来自多个语言服务器。

## 它和搜索内容的区别

- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/) 更适合模糊文本搜索。
- `LSP查询` 更适合语义级导航。

比如：

- 想找某个函数在哪定义，用 `definition`
- 想找所有调用点，用 `references`
- 想看文件结构，用 `documentSymbol`

## 常见误区

### 把它当成全文检索

如果你只有关键词，没有明确代码位置，先用文本搜索通常更快。

### 忽视 1-based 位置

这个工具的 `line` 和 `character` 都是从 1 开始计。

## 实施建议

- `LSP查询` 是语义导航，不是字符串搜索。
- 不同操作对位置参数的要求不同。
- 返回结果可能来自多个 LSP 服务端，不一定只有一份。

## 延伸阅读

- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [系统架构](/docs/zh-CN/concepts/architecture/)
