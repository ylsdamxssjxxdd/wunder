---
title: 界面协同工具
summary: a2ui、计划面板、问询面板负责把模型执行过程变成前端可展示、可交互的界面状态，而不是只输出一段文本。
read_when:
  - 你要理解 Wunder 前端为什么能展示计划和问询分流
  - 你要知道哪些工具专门用来驱动界面
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# 界面协同工具

把模型执行过程可视化的界面驱动工具。

---

## 工具概览

界面协同工具包含三个工具：

| 工具名 | 说明 | 别名 |
|--------|------|------|
| `a2ui` | 发送结构化 UI 消息 | - |
| `计划面板` | 展示步骤化执行计划 | `update_plan` |
| `问询面板` | 让用户选择实现路线 | `question_panel`、`ask_panel` |

---

## a2ui

### 功能说明

把结构化 UI 消息发送给前端，用于展示各种 UI 组件。

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `uid` | string | ❌ | 唯一标识符 |
| `a2ui` | object | ✅ | UI 载荷 |
| `content` | string | ❌ | 内容文本 |

---

## 计划面板

### 功能说明

展示步骤化执行计划，让用户了解当前进度和后续步骤。

**别名**：
- `update_plan`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `explanation` | string | ❌ | 计划说明 |
| `plan` | array | ✅ | 计划步骤数组 |

每个 plan 项包含：
| 字段 | 类型 | 说明 |
|------|------|------|
| `step` | string | 步骤名称 |
| `status` | string | 状态：`pending`/`in_progress`/`completed` |

**系统自动处理**：
- 只保留一个 `in_progress` 项
- 其余会自动回落为 `pending`

### 使用示例

```json
{
  "explanation": "正在重构项目结构",
  "plan": [
    {
      "step": "分析当前目录结构",
      "status": "completed"
    },
    {
      "step": "创建新的模块目录",
      "status": "in_progress"
    },
    {
      "step": "移动文件到新位置",
      "status": "pending"
    },
    {
      "step": "更新导入语句",
      "status": "pending"
    }
  ]
}
```

---

## 问询面板

### 功能说明

让前端展示一个可选路线面板，让用户选择实现方案。

**别名**：
- `question_panel`
- `ask_panel`

### 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `question` | string | ✅ | 问题文本 |
| `routes` | array | ✅ | 可选路线数组 |
| `multiple` | boolean | ❌ | 是否允许多选，默认 false |

每个 route 项包含：
| 字段 | 类型 | 说明 |
|------|------|------|
| `label` | string | 选项标签 |
| `description` | string | 选项说明 |
| `recommended` | boolean | 是否推荐 |

### 使用示例

```json
{
  "question": "你想采用哪种重构方案？",
  "multiple": false,
  "routes": [
    {
      "label": "方案 A：渐进式重构",
      "description": "逐步迁移，风险较低",
      "recommended": true
    },
    {
      "label": "方案 B：大爆炸重构",
      "description": "一次性完成，风险较高",
      "recommended": false
    }
  ]
}
```

---

## 适用场景

✅ **适合使用界面协同工具**：
- 展示执行计划和进度
- 在多个实现路径之间让用户选路
- 把结构化界面状态推给前端
- 让用户参与决策过程

---

## 与最终回复的区别

| 特性 | 界面协同工具 | [最终回复](/docs/zh-CN/tools/final-response/) |
|------|-------------|--------------------------------|
| 目标 | 过程可视化 | 结束本轮 |
| 时机 | 执行过程中 | 最后一步 |
| 作用 | 给界面补结构化状态 | 返回最终答案 |

---

## 注意事项

1. **不是替代最终回复**：
   - 界面协同工具负责过程展示
   - 最终仍然需要用 `最终回复` 结束

2. **状态管理**：
   - `计划面板` 的状态会自动规范化
   - 只保留一个 `in_progress` 项

3. **前端展示**：
   - 这些工具最适合配合用户侧前端和管理端调试面板
   - 能更好地理解执行过程

---

## 延伸阅读

- [用户侧前端](/docs/zh-CN/surfaces/frontend/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
