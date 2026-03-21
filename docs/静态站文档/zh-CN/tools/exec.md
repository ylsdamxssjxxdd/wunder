---
title: 执行命令
summary: 在工作区中执行系统命令，支持编译、测试、运行脚本，受安全策略和预算约束。
read_when:
  - 你需要在工作区中运行命令
  - 你想了解命令执行的安全边界
source_docs:
  - src/services/tools/catalog.rs
  - config/wunder-example.yaml
---

# 执行命令

在工作区中执行系统命令，如编译、测试、运行脚本等。

---

## 功能说明

`执行命令` 用于在当前工作区中执行系统命令，适用于：
- 编译项目
- 运行测试
- 执行脚本
- 产物检查

**别名**：
- `execute_command`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `content` | string | ✅ | 要执行的命令 |
| `workdir` | string | ❌ | 工作目录，默认为工作区根目录 |
| `timeout_s` | integer | ❌ | 超时时间（秒） |
| `dry_run` | boolean | ❌ | 预演模式 |
| `time_budget_ms` | integer | ❌ | 时间预算（毫秒） |
| `output_budget_bytes` | integer | ❌ | 输出预算（字节） |
| `max_commands` | integer | ❌ | 最大命令数 |

---

## 使用示例

### 简单命令
```json
{
  "content": "ls -la"
}
```

### 指定工作目录
```json
{
  "content": "cargo build",
  "workdir": "src"
}
```

### 带超时设置
```json
{
  "content": "npm install",
  "timeout_s": 300
}
```

### 预演模式
```json
{
  "content": "rm -rf temp/",
  "dry_run": true
}
```

---

## 安全边界

`执行命令` 不是完全自由的，受以下约束：

| 约束项 | 说明 |
|--------|------|
| `allow_commands` | 允许的命令列表 |
| `allow_paths` | 允许的路径列表 |
| `deny_globs` | 拒绝的文件模式 |
| 执行环境 | 本机执行或沙盒执行 |

---

## 与 ptc 的对比

| 特性 | 执行命令 | [ptc](/docs/zh-CN/tools/ptc/) |
|------|----------|--------------------------------|
| 适用场景 | 运行已有命令、脚本、构建 | 先形成脚本再执行 |
| 目标 | 执行外部命令 | 程序化执行 |
| 推荐使用 | 执行已有命令 | 复杂逻辑、图表生成 |

---

## 常见场景

### 编译 Rust 项目
```json
{
  "content": "cargo build --release"
}
```

### 运行 Python 脚本
```json
{
  "content": "python script.py"
}
```

### 安装 npm 依赖
```json
{
  "content": "npm install"
}
```

### 运行测试
```json
{
  "content": "cargo test"
}
```

---

## 注意事项

1. **预算控制很重要**：
   - 长命令和海量输出可能拖爆上下文
   - 使用 `output_budget_bytes` 限制输出大小

2. **文件编辑不用命令**：
   - 文件编辑优先用 `写入文件` 或 `应用补丁`
   - 不要用 `echo` 等命令创建文件

3. **安全第一**：
   - 危险命令先用 `dry_run` 预览
   - 注意命令的影响范围

---

## 延伸阅读

- [ptc](/docs/zh-CN/tools/ptc/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
