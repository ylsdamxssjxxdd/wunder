---
title: 应用补丁
summary: `apply_patch` 的精确编辑语义、成功返回和 patch 失败码。
read_when:
  - 你要做小范围、可审查、可回放的精确修改
source_docs:
  - src/services/tools/apply_patch_tool.rs
  - src/services/tools/tool_apply_patch.lark
updated_at: 2026-04-10
---

# 应用补丁

`apply_patch` 是当前最适合做“少量文件、少量块、明确上下文”的编辑工具。

它的定位很明确：

- 不是整文件写入工具
- 不是命令执行工具
- 是结构化、小步、可验证的精确修改工具

## 输入不是 JSON patch，而是 grammar 文本

最小示例：

```text
*** Begin Patch
*** Update File: src/main.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
```

模型侧常用参数只有两个：

- `input`
- `dry_run`

## 成功返回

```json
{
  "ok": true,
  "action": "apply_patch",
  "state": "completed",
  "summary": "Applied patch touching 2 files.",
  "data": {
    "changed_files": 2,
    "added": 1,
    "updated": 1,
    "deleted": 0,
    "moved": 0,
    "hunks_applied": 3,
    "files": [
      {
        "action": "update",
        "path": "src/main.rs",
        "to_path": null,
        "hunks": 1
      }
    ],
    "lsp": [
      {
        "path": "C:/.../src/main.rs",
        "state": {
          "enabled": true,
          "matched": true,
          "touched": true
        }
      }
    ]
  }
}
```

## `dry_run`

```json
{
  "ok": true,
  "action": "apply_patch",
  "state": "dry_run",
  "summary": "Validated patch touching 2 files without applying it.",
  "data": {
    "dry_run": true,
    "changed_files": 2,
    "added": 1,
    "updated": 1,
    "deleted": 0,
    "moved": 0,
    "hunks_applied": 3,
    "files": [ ... ],
    "lsp": []
  }
}
```

## 失败返回

`apply_patch` 失败时虽然也会落到统一失败骨架，但错误码比普通文件工具更细：

- `PATCH_LIMIT_INPUT_TOO_LARGE`
- `PATCH_FORMAT_EMPTY_PATCH`
- `PATCH_LIMIT_TOO_MANY_FILE_OPS`
- `PATCH_LIMIT_TOO_MANY_CHUNKS`
- `PATCH_RUNTIME_TASK_FAILED`

此外还会有解析、路径越界、目标冲突、上下文不匹配等 patch 级错误。

## 什么时候用它，什么时候别用它

适合：

- 改几行代码
- 增加一个小函数
- 调整几个独立文件

不适合：

- 整文件重写
- 大批量生成文档或资源
- 需要先运行脚本再产出结果的修改

这些场景分别考虑：

- 整文件重写：`write_file`
- 先跑命令：`execute_command`

## 与 `write_file` 的区别

- `apply_patch`：保留上下文，便于审查
- `write_file`：直接覆盖最终内容

如果你已经知道“整个文件的新内容就应该是什么”，用 `write_file` 更直接。  
如果你只想做精确改动，`apply_patch` 更稳。
