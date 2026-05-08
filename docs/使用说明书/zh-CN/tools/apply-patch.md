---
title: 应用补丁
summary: `apply_patch` 的精确编辑语义、成功返回和 patch 失败码。
read_when:
  - 你要做小范围、可审查、可回放的精确修改
source_docs:
  - src/services/tools/apply_patch_tool.rs
  - src/services/tools/tool_apply_patch.lark
updated_at: 2026-05-08
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

## 推荐给弱模型的最小流程

按下面顺序做，成功率最高：

1. 先 `read_file` 读取目标文件或精确片段
2. 一次只构造一个很小的 patch
3. 如果不确定上下文是否稳定，先加 `dry_run`
4. 预演通过后，再去掉 `dry_run` 正式提交
5. 如果修改跨很多分散区域，或已经接近整文件改写，直接换 `write_file`

## Update File 的硬规则

- `@@` 之后每一行都必须以前缀开头：空格 / `-` / `+`
- 空白上下文行不能留空，必须写成“只有一个前导空格的一行”
- 真正修改某一行时，必须明确写成 `-旧行` 再 `+新行`
- 不要把旧行和新行都写成普通上下文行
- 不要复制 `read_file` 输出里的 `>>> 路径`、`N: ` 行号或分隔符
- 每处修改前后优先保留 2-3 行当前文件中的原始上下文

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

`dry_run` 只做解析和匹配预演，不会落盘。以下情况优先使用：

- 刚从 `read_file` 拼出第一个 patch，还不确定格式是否正确
- 修改位置比较敏感，担心上下文匹配失败
- 能力较弱的模型第一次尝试该工具

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

## 常见失败原因

- 把补丁正文又包进了一层 JSON 或 Markdown 代码块
- `@@` 后直接粘贴原文件内容，没有给每一行加前缀
- 把空白上下文行写成了真正的空行
- 把旧行和新行都写成空格前缀的上下文行
- 复制了 `read_file` 的展示行号或 `>>> 路径`
- 一次性修改太多分散区域，超出 `apply_patch` 的设计范围
- `-旧行` 和 `+新行` 实际写成了同一个内容，导致补丁“成功执行但没有任何变化”

## 典型修正建议

当前工具在部分 `PATCH_CONTEXT_NOT_FOUND` 场景下，会尽量给出更具体的修正方向，而不只是泛泛地说“上下文没找到”。例如：

- 如果前一个变更块只有上下文、没有实际改动，提示会指出这是“空 hunk”
- 如果后一个变更块复用了前一个空 hunk 的同一组锚点，提示会指出这是“重复锚点/多写了一个 @@ 块”
- 如果上下文行和删除行重复出现，提示会指出“要删除的行不应同时作为上下文行”
- 如果补丁看起来有改动，但最终文件完全没变化，提示会进一步区分“只有上下文、没有真实变更”和“旧行/新行实际相同”

遇到这类提示时，优先做下面几件事：

- 删除没有任何 `+` / `-` 的空 hunk
- 把同一处插入或替换内容合并回一个 hunk
- 不要让两个连续 hunk 复用同一组锚点
- 纯插入 hunk 尽量同时保留头部和尾部未修改原文，避免只靠单侧锚点定位

## 什么时候用它，什么时候别用它

当前版本相比最早实现已经放宽了一些范围限制，允许单次补丁覆盖更多分散区域；但它依然更适合“中小批量精确编辑”，而不是整文件级重写。

适合：

- 改几行代码
- 增加一个小函数
- 调整几个独立文件

不适合：

- 整文件重写
- 大批量生成文档或资源
- 需要先运行脚本再产出结果的修改
- 跨很多分散区域的大型重构

这些场景分别考虑：

- 整文件重写：`write_file`
- 先跑命令：`execute_command`

## 与 `write_file` 的区别

- `apply_patch`：保留上下文，便于审查
- `write_file`：直接覆盖最终内容

如果你已经知道“整个文件的新内容就应该是什么”，用 `write_file` 更直接。  
如果你只想做精确改动，`apply_patch` 更稳。
