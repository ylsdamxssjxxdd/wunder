---
title: CLI 使用
summary: 需要终端执行、脚本化接入或开发调试时，再看 `wunder-cli`。
read_when:
  - 你想在终端中直接使用 wunder
  - 你更关注开发调试、脚本任务和工作区驱动执行
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
---

# CLI 使用

如果你主要在终端里工作，这页先看。

`wunder-cli` 是 Wunder 的命令行形态，适合开发者、本地任务、脚本化调用和工作区驱动执行。

---

## 什么时候选 CLI？

| 场景 | 选 CLI |
|------|--------|
| 你要做编程类任务或文件处理 | ✅ |
| 你要在终端里观察事件、工具调用和产物 | ✅ |
| 你要接脚本、CI、批处理或自动化流程 | ✅ |
| 你需要 JSONL 事件流或 TUI 交互 | ✅ |
| 你只是想直接日常使用，不想碰终端 | ❌ 选 Desktop |

---

## CLI 是什么？

CLI 不是「功能更少的 Desktop」，它是一个**终端优先的独立运行形态**：

```
┌─────────────────────────────────────┐
│     wunder-cli                      │
│  ┌───────────────────────────────┐ │
│  │  TUI 交互界面（可选）          │ │
│  │  - 类似 Codex 的终端体验       │ │
│  │  - 实时显示事件流              │ │
│  └───────────────────────────────┘ │
│  ↓ 命令行驱动                       │
│  ┌───────────────────────────────┐ │
│  │  Rust 核心引擎                │ │
│  │  - 调度器                     │ │
│  │  - 工具链                     │ │
│  │  - 当前目录作为工作区          │ │
│  └───────────────────────────────┘ │
└─────────────────────────────────────┘
```

### CLI 核心能力

- ✅ 完整的智能体调度和工具链
- ✅ TUI 交互界面（类 Codex 体验）
- ✅ JSONL 输出，便于管道和自动化
- ✅ 会话管理（fork、compact、resume）
- ✅ 渠道接入（飞书、微信、QQ、XMPP 等）
- ✅ 调试工具（debug-config、statusline）

---

## 5 步上手 CLI

### 1. 安装 CLI

**从源码编译（需要 Rust 1.70+）：**
```bash
git clone <repo-url>
cd wunder
cargo build --release
```

编译后，可执行文件在 `./target/release/wunder-cli`

### 2. 配置模型

创建或编辑 `~/.wunder/config.yaml`：

```yaml
llm:
  models:
    - name: gpt-4o
      api_key: your-api-key
      endpoint: https://api.openai.com/v1
      max_context: 128000
      max_rounds: 20
```

### 3. 启动交互模式

```bash
# 进入当前目录作为工作区
cd my-project
wunder-cli
```

你会看到：
```
wunder-cli v0.1.0
工作区: /path/to/my-project
输入 /help 查看命令
> 
```

### 4. 发起第一次任务

```
> 帮我列出当前目录的文件
```

你会看到：
- 模型思考
- 调用「列出文件」工具
- 展示文件列表
- 给出最终回复

### 5. 探索 CLI 命令

试试这些内置命令：

| 命令 | 说明 |
|------|------|
| `/help` | 显示所有可用命令 |
| `/fork` | 分叉当前会话 |
| `/compact` | 压缩会话历史 |
| `/resume <id>` | 恢复历史会话 |
| `/debug-config` | 显示当前配置 |
| `/statusline` | 显示运行状态 |

---

## CLI 特有能力

### TUI 交互界面

启动时加上 `--tui` 参数：
```bash
wunder-cli --tui
```

你会得到类似 Codex 的终端体验：
- 左侧：会话列表
- 中间：聊天区域
- 右侧：工具调用和状态
- 底部：输入框

### 会话管理

```
> /fork          # 分叉当前会话（保留历史）
> /compact       # 压缩会话历史（节省 token）
> /resume abc123 # 恢复会话 abc123
> /list          # 列出所有会话
```

### 脚本化和自动化

**单次执行模式：**
```bash
wunder-cli --execute "帮我写一个 Hello World"
```

**JSONL 输出（便于管道）：**
```bash
wunder-cli --jsonl --execute "列出文件" | jq .
```

**从文件读取任务：**
```bash
wunder-cli --file task.txt
```

### 工作区驱动

CLI 默认使用**当前目录**作为工作区：
```bash
cd my-project
wunder-cli  # 工作区就是 my-project
```

也可以指定工作区：
```bash
wunder-cli --workspace /path/to/workspace
```

---

## 常见问题

**Q: CLI 需要联网吗？**  
A: 模型调用需要联网，但工具执行可以纯本地。

**Q: 能和 Desktop/Server 配合用吗？**  
A: 可以！三者共享同一套核心引擎，工作区格式兼容。

**Q: 如何在 CI/CD 中使用？**  
A: 用 `--execute` + `--jsonl` 模式，便于脚本集成。

**Q: 会话数据存在哪里？**  
A: 默认在 `~/.wunder/sessions/`，可以通过配置修改。

---

## 下一步

- 想看所有工具？→ [工具总览](/docs/zh-CN/tools/)
- 要做自动化？→ [接入概览](/docs/zh-CN/integration/)
- 遇到问题？→ [故障排查](/docs/zh-CN/help/troubleshooting/)
- 想深入系统核心？→ [核心概览](/docs/zh-CN/concepts/)
