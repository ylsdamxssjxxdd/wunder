---
名称: mcp-builder
描述: 用于创建高质量 MCP（Model Context Protocol）服务器的指南，使 LLM 能通过设计良好的工具与外部服务交互。适用于用 Python（FastMCP）或 Node/TypeScript（MCP SDK）构建 MCP 服务器、对接外部 API 或服务时。
---

# MCP 服务器开发指南

## 概述

创建 MCP（Model Context Protocol）服务器，让 LLM 通过设计良好的工具与外部服务交互。MCP 服务器的质量取决于它是否能帮助 LLM 高效完成真实任务。

---

# 流程

## 高层工作流

构建高质量 MCP 服务器一般包含四个阶段：

### 阶段 1：深入调研与规划

#### 1.1 了解现代 MCP 设计

**API 覆盖 vs. 工作流工具：**
在完整 API 覆盖与专用工作流工具之间取得平衡。工作流工具适合特定任务更便捷，而完整覆盖让智能体具备更高组合灵活性。性能因客户端而异：有的客户端更适合用代码组合基础工具，有的更适合高层工作流。若不确定，优先完整 API 覆盖。

**工具命名与可发现性：**
清晰、可描述的工具名称有助于智能体快速找到合适工具。使用一致前缀（如 `github_create_issue`、`github_list_repos`），以动作导向命名。

**上下文管理：**
工具说明要简洁，返回结果需支持过滤/分页，确保输出聚焦且相关。部分客户端支持代码执行，可进一步帮助智能体处理与筛选数据。

**可行动的错误信息：**
错误信息应提供明确的解决建议与下一步指引。

#### 1.2 研究 MCP 协议文档

**查阅 MCP 规范：**

先通过 sitemap 查找相关页面：`https://modelcontextprotocol.io/sitemap.xml`

再用 `.md` 后缀获取 Markdown 格式（例如 `https://modelcontextprotocol.io/specification/draft.md`）。

重点关注：
- 规范概览与架构
- 传输机制（streamable HTTP、stdio）
- 工具、资源与提示词定义

#### 1.3 研究框架文档

**推荐技术栈：**
- **语言**：TypeScript（SDK 支持成熟，兼容性好。模型也更擅长生成 TS 代码，得益于广泛生态、静态类型与完善的 lint 工具）
- **传输**：远程服务优先 streamable HTTP，使用无状态 JSON（更易扩展与维护）；本地服务使用 stdio

**加载框架文档：**

- **MCP Best Practices**：[查看最佳实践](./reference/mcp_best_practices.md) - 核心指南

**TypeScript（推荐）**：
- **TypeScript SDK**：通过 WebFetch 加载 `https://raw.githubusercontent.com/modelcontextprotocol/typescript-sdk/main/README.md`
- [TypeScript 指南](./reference/node_mcp_server.md) - TS 模式与示例

**Python：**
- **Python SDK**：通过 WebFetch 加载 `https://raw.githubusercontent.com/modelcontextprotocol/python-sdk/main/README.md`
- [Python 指南](./reference/python_mcp_server.md) - Python 模式与示例

#### 1.4 规划实现

**理解 API：**
查阅服务 API 文档，明确关键端点、鉴权方式与数据模型。必要时使用 WebSearch/WebFetch。

**工具选择：**
优先覆盖常用端点，从最常用的操作开始实现。

---

### 阶段 2：实现

#### 2.1 搭建项目结构

参考语言特定指南：
- [TypeScript 指南](./reference/node_mcp_server.md) - 项目结构、package.json、tsconfig.json
- [Python 指南](./reference/python_mcp_server.md) - 模块组织与依赖

#### 2.2 实现核心基础设施

创建共享工具：
- API 客户端与鉴权
- 错误处理工具
- 响应格式化（JSON/Markdown）
- 分页支持

#### 2.3 实现工具

针对每个工具：

**输入 Schema：**
- TypeScript 使用 Zod，Python 使用 Pydantic
- 添加约束与清晰说明
- 在字段描述中给出示例

**输出 Schema：**
- 尽量定义 `outputSchema` 提供结构化数据
- 使用工具响应中的 `structuredContent`（TypeScript SDK 功能）
- 便于客户端理解与处理输出

**工具描述：**
- 功能简述
- 参数说明
- 返回类型说明

**实现要点：**
- I/O 使用 async/await
- 规范错误处理并提供可行动信息
- 支持分页
- 使用现代 SDK 时同时返回文本与结构化数据

**注解：**
- `readOnlyHint`: true/false
- `destructiveHint`: true/false
- `idempotentHint`: true/false
- `openWorldHint`: true/false

---

### 阶段 3：评审与测试

#### 3.1 代码质量

检查：
- 无重复代码（DRY）
- 错误处理一致
- 类型覆盖完整
- 工具描述清晰

#### 3.2 构建与测试

**TypeScript：**
- 运行 `npm run build` 验证编译
- 使用 MCP Inspector：`npx @modelcontextprotocol/inspector`

**Python：**
- 语法检查：`python -m py_compile your_server.py`
- 使用 MCP Inspector

更详细的测试与质量清单见语言特定指南。

---

### 阶段 4：创建评估

实现 MCP 服务器后，需要创建评估以测试效果。

**完整评估指南请加载** [Evaluation Guide](./reference/evaluation.md)。

#### 4.1 了解评估目的

评估用于验证 LLM 能否通过你的 MCP 服务器回答真实复杂问题。

#### 4.2 创建 10 个评估问题

按以下流程创建：

1. **工具检查**：列出可用工具并理解能力
2. **内容探索**：使用只读操作探索数据
3. **问题生成**：创建 10 个复杂且真实的问题
4. **答案验证**：你自己完成问题并核对答案

#### 4.3 评估要求

每个问题必须：
- **独立**：不依赖其他问题
- **只读**：无需破坏性操作
- **复杂**：需要多次工具调用与深入探索
- **真实**：人类真实会关心的场景
- **可验证**：答案清晰且可用字符串对比验证
- **稳定**：答案不会随时间变化

#### 4.4 输出格式

创建 XML 文件，结构如下：

```xml
<evaluation>
  <qa_pair>
    <question>Find discussions about AI model launches with animal codenames. One model needed a specific safety designation that uses the format ASL-X. What number X was being determined for the model named after a spotted wild cat?</question>
    <answer>3</answer>
  </qa_pair>
<!-- More qa_pairs... -->
</evaluation>
```

---

# 参考文件

## 文档库

按需加载以下资源：

### MCP 核心文档（优先加载）
- **MCP 协议**：从 sitemap `https://modelcontextprotocol.io/sitemap.xml` 开始，再加载具体 `.md` 页面
- [MCP 最佳实践](./reference/mcp_best_practices.md) - 通用 MCP 规范，包括：
  - 服务器与工具命名规范
  - 响应格式（JSON vs Markdown）指南
  - 分页最佳实践
  - 传输方式选择（streamable HTTP vs stdio）
  - 安全与错误处理标准

### SDK 文档（阶段 1/2 加载）
- **Python SDK**：`https://raw.githubusercontent.com/modelcontextprotocol/python-sdk/main/README.md`
- **TypeScript SDK**：`https://raw.githubusercontent.com/modelcontextprotocol/typescript-sdk/main/README.md`

### 语言实现指南（阶段 2 加载）
- [Python 实现指南](./reference/python_mcp_server.md) - 完整 Python/FastMCP 指南：
  - 服务器初始化模式
  - Pydantic 模型示例
  - 使用 `@mcp.tool` 注册工具
  - 完整示例
  - 质量检查清单

- [TypeScript 实现指南](./reference/node_mcp_server.md) - 完整 TypeScript 指南：
  - 项目结构
  - Zod Schema 模式
  - 使用 `server.registerTool` 注册工具
  - 完整示例
  - 质量检查清单

### 评估指南（阶段 4 加载）
- [Evaluation Guide](./reference/evaluation.md) - 评估创建指南，包括：
  - 问题创建规范
  - 答案验证策略
  - XML 格式说明
  - 示例问答
  - 使用提供脚本运行评估
