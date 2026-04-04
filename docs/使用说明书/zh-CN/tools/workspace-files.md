---
title: 文件与工作区工具
summary: 文件与工作区工具是最常用的内置工具，覆盖列出文件、搜索内容、读取文件、写入文件四大核心功能。
read_when:
  - 你需要在工作区中操作文件
  - 你想了解文件工具的正确使用顺序
source_docs:
  - src/services/tools/catalog.rs
  - docs/API文档.md
---

# 文件与工作区工具

这是最常用的一组内置工具，包含：
- **列出文件**：浏览目录结构
- **搜索内容**：在文件中搜索
- **读取文件**：读取文件内容
- **写入文件**：创建或覆盖文件

---

## 推荐工作流

使用文件工具的推荐顺序：
1. **先列目录** → 了解文件结构
2. **再搜索** → 定位目标内容
3. **然后读取** → 查看具体内容
4. **最后写入** → 进行修改

这套顺序比直接读取整个文件更稳定，也更省上下文。

---

## 列出文件

### 功能说明

浏览目录结构，查看文件树。

**别名**：
- `list_files`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `path` | string | ❌ | 目标路径，默认为工作区根目录 |
| `max_depth` | integer | ❌ | 最大遍历深度，默认 3 |
| `file_pattern` | string | ❌ | 文件匹配模式 |

### 使用示例

#### 列出根目录
```json
{
  "path": ".",
  "max_depth": 2
}
```

#### 列出 src 目录
```json
{
  "path": "src",
  "max_depth": 3
}
```

---

## 搜索内容

### 功能说明

在工作区中按关键词搜索代码、配置或日志。

推荐按 `rg` 的思路组织搜索参数：
- 用 `path` 先缩小目录范围
- 用 `pattern`/`query` 写字面量或正则
- 用 `glob`/`file_pattern`/`type` 过滤文件
- 用 `context` 或 `-A`/`-B`/`-C` 请求上下文

**别名**：
- `search_content`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `query` | string | ❌ | 主查询输入；默认按 `literal` 匹配。若多词精确短语无结果，工具可能自动回退为按词检索 |
| `pattern` | string | ❌ | 推荐的 `rg` 风格输入；仅传 `pattern` 且未显式设置 `query_mode`/`-F` 时，默认按 `regex` 处理 |
| `path` | string | ❌ | 搜索路径，默认为工作区根目录 |
| `file_pattern` | string | ❌ | 文件匹配模式，如 `*.rs` |
| `glob` | string | ❌ | 推荐的 `rg` 风格别名，等价于 `file_pattern` |
| `type` | string/array | ❌ | 常见类型快捷过滤，如 `rust`、`rs`、`ts`、`tsx`、`js`、`py`、`md` |
| `query_mode` | string | ❌ | 搜索模式：`literal`/`regex`。省略时，`query` 默认按 `literal`，`pattern` 默认按 `regex` |
| `regex` | boolean | ❌ | `true` 等价于 `query_mode=regex` |
| `fixed_strings` / `-F` | boolean | ❌ | 强制按字面量匹配，等价于 `query_mode=literal` |
| `case_sensitive` | boolean | ❌ | 是否区分大小写，默认 false |
| `ignore_case` / `-i` | boolean | ❌ | `rg` 风格别名，`true` 表示忽略大小写 |
| `max_depth` | integer | ❌ | 最大搜索深度，默认 5 |
| `max_files` | integer | ❌ | 最大文件数 |
| `max_matches` | integer | ❌ | 最大匹配数 |
| `max_count` / `head_limit` | integer | ❌ | `rg` 风格别名，等价于 `max_matches` |
| `context` / `-C` | integer | ❌ | 同时设置前后上下文行数 |
| `context_before` | integer | ❌ | 匹配前显示行数 |
| `context_after` | integer | ❌ | 匹配后显示行数 |
| `-B` | integer | ❌ | `rg` 风格别名，等价于 `context_before` |
| `-A` | integer | ❌ | `rg` 风格别名，等价于 `context_after` |
| `dry_run` | boolean | ❌ | 预演模式 |
| `time_budget_ms` | integer | ❌ | 时间预算（毫秒） |
| `output_budget_bytes` | integer | ❌ | 输出预算（字节） |

补充说明：
- `query` 适合自然语言或固定短语检索，优先保证精确度；必要时才自动做按词回退。
- `pattern` 更适合 `rg`/正则心智模型；若需要固定字符串语义，请显式传 `-F` 或 `query_mode=literal`。
- 返回结果会优先按相关性排序，并在 `summary`/`meta.search` 中提供实际采用的策略、尝试过的查询计划和下一步提示。

### 使用示例

#### 简单搜索
```json
{
  "query": "fn main",
  "file_pattern": "*.rs"
}
```

#### 推荐的 rg 风格搜索
```json
{
  "pattern": "turn_terminal|event title|resolve.*title",
  "path": "src",
  "glob": "*.rs",
  "context": 2,
  "max_count": 50
}
```

#### 固定字符串模式
```json
{
  "pattern": "foo.bar",
  "-F": true,
  "glob": "*.rs"
}
```

#### 按类型过滤并带上下文
```json
{
  "pattern": "execute_tool",
  "type": "rust",
  "-B": 3,
  "-A": 5
}
```

---

## 读取文件

### 功能说明

读取文件内容，支持切片读取，不鼓励大文件整段读取。

**别名**：
- `read_file`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `path` | string | ✅ | 文件路径 |
| `start_line` | integer | ❌ | 起始行号 |
| `end_line` | integer | ❌ | 结束行号 |
| `line_ranges` | array | ❌ | 行范围数组 |
| `mode` | string | ❌ | 读取模式：`full`/`indentation` |

### 使用示例

#### 读取整个文件
```json
{
  "path": "src/main.rs"
}
```

#### 读取指定行范围
```json
{
  "path": "src/main.rs",
  "start_line": 1,
  "end_line": 50
}
```

#### 读取多个不连续范围
```json
{
  "path": "src/main.rs",
  "line_ranges": [[1, 20], [50, 100]]
}
```

---

## 写入文件

### 功能说明

创建新文件或全量覆盖现有文件。

**别名**：
- `write_file`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `path` | string | ✅ | 文件路径 |
| `content` | string | ✅ | 文件内容 |
| `dry_run` | boolean | ❌ | 预演模式 |

### 使用示例

#### 创建新文件
```json
{
  "path": "src/hello.rs",
  "content": "fn main() {\n    println!(\"Hello, world!\");\n}\n"
}
```

#### 全量覆盖文件
```json
{
  "path": "config.yaml",
  "content": "server:\n  port: 8080\n"
}
```

---

## 工具对比

| 场景 | 推荐工具 | 说明 |
|------|----------|------|
| 单文件全量替换 | 写入文件 | 适用于新文件或短文件 |
| 多位置精确编辑 | [应用补丁](/docs/zh-CN/tools/apply-patch/) | 结构化修改，更安全 |
| 查找关键词 | 搜索内容 | 快速定位，省上下文 |
| 浏览目录 | 列出文件 | 了解文件结构 |
| 读取局部内容 | 读取文件 | 切片读取，不浪费 |

---

## 注意事项

1. **原子写入**：
   - `写入文件` 采用临时文件 + rename 策略
   - 降低异常中断与并发覆盖风险

2. **预算控制**：
   - `搜索内容` 支持预算参数
   - 避免大搜索拖慢系统

3. **工具选型**：
   - 单文件全量替换用 `写入文件`
   - 多位置编辑用 `应用补丁`
   - 不要混用

---

## 延伸阅读

- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [LSP查询](/docs/zh-CN/tools/lsp/)
- [执行命令](/docs/zh-CN/tools/exec/)
- [工作区 API](/docs/zh-CN/integration/workspace-api/)
