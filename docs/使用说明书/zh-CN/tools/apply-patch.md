---
title: 应用补丁
summary: 应用补丁是做多文件、多位置结构化编辑的主力工具，比多次零碎写文件更稳定、更安全。
read_when:
  - 你要跨多个文件做精确编辑
  - 你在调试补丁格式为什么没被正确应用
source_docs:
  - src/services/tools/apply_patch_tool.rs
  - src/services/tools/catalog.rs
---

# 应用补丁

多文件、多位置结构化编辑的主力工具。

---

## 功能说明

`应用补丁` 适合多文件和多位置修改场景，采用临时文件 + rename 策略保证原子写入。

**别名**：
- `apply_patch`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `input` | string | ✅ | 完整补丁文本 |
| `dry_run` | boolean | ❌ | 预演模式，不实际写入 |

---

## 补丁格式

### 基本结构

```text
*** Begin Patch
*** Update File: src/main.rs
@@
-    println!("Hello");
+    println!("Hello, world!");
*** End Patch
```

### 多文件修改

```text
*** Begin Patch
*** Update File: src/main.rs
@@
-    println!("Hello");
+    println!("Hello, world!");
*** Update File: src/utils.rs
@@
-const VERSION = "1.0";
+const VERSION = "2.0";
*** End Patch
```

### 同一文件多位置修改

```text
*** Begin Patch
*** Update File: src/main.rs
@@
-fn greet() {
+fn greet(name: &str) {
@@
-    println!("Hello");
+    println!("Hello, {}!", name);
*** End Patch
```

---

## 使用示例

### 单文件修改

```json
{
  "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-fn main() {\n+fn main() {\n+    println!(\"Starting...\");\n*** End Patch",
  "dry_run": false
}
```

### 预演模式

```json
{
  "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-const DEBUG = true;\n+const DEBUG = false;\n*** End Patch",
  "dry_run": true
}
```

---

## 为什么用补丁而不是写入文件

| 特性 | 应用补丁 | 写入文件 |
|------|----------|----------|
| 多文件修改 | ✅ 单个工具调用 | ❌ 需要多次调用 |
| 多位置修改 | ✅ 单个工具调用 | ❌ 需要多次调用 |
| 原子性 | ✅ 全成功或全失败 | ⚠️ 单个文件原子 |
| 上下文定位 | ✅ 基于上下文匹配 | ❌ 全量替换 |
| 代码审查 | ✅ 更易审查 | ⚠️ 全量对比 |

---

## dry_run 的用途

1. **验证补丁语法**：检查补丁格式是否正确
2. **验证目标文件**：确认目标文件存在且可匹配
3. **预览修改**：查看会改动哪些文件，再决定是否落盘

---

## 常见误区

### 把它当普通文本替换
❌ 补丁依赖上下文定位，不是单纯的全局替换字符串。

### 补丁写得太松
⚠️ 上下文过少时，命中位置会更脆弱，建议提供足够的上下文。

### 单文件全量生成还用补丁
❌ 如果是简单新文件，直接用 `写入文件` 更干净。

---

## 注意事项

1. **原子写入**：采用临时文件 + rename 策略
2. **上下文匹配**：补丁需要足够的上下文来准确定位
3. **dry_run 推荐**：复杂修改先用 dry_run 预览
4. **工具选型**：
   - 多位置、多文件修改 → 应用补丁
   - 单文件全量覆盖 → 写入文件

---

## 延伸阅读

- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
- [执行命令](/docs/zh-CN/tools/exec/)
