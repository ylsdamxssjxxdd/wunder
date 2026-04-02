---
title: ptc (Python 程序化执行)
summary: ptc 会把 Python 脚本内容写入工作区临时目录并执行，适合图表、表格、程序化中间产物这类先生成再运行的场景。
read_when:
  - 你要理解 ptc 和执行命令的分工
  - 你要生成脚本产物而不是直接跑已有命令
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
---

# ptc (Python 程序化执行)

先生成脚本再执行的程序化工具。

---

## 功能说明

`ptc` (Programmatic Tool Call) 让模型先给出一段 Python 脚本内容，再由系统把脚本写入临时目录并执行。

**别名**：
- `programmatic_tool_call`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `filename` | string | ❌ | 文件名，不允许路径段 |
| `workdir` | string | ❌ | 工作目录 |
| `content` | string | ✅ | Python 脚本内容 |

---

## 使用示例

### 生成图表

```json
{
  "filename": "plot.py",
  "content": "import matplotlib.pyplot as plt\nimport numpy as np\n\nx = np.linspace(0, 10, 100)\ny = np.sin(x)\n\nplt.plot(x, y)\nplt.savefig('plot.png')\nprint('Plot saved to plot.png')"
}
```

### 数据处理

```json
{
  "content": "import pandas as pd\n\ndata = pd.read_csv('data.csv')\nsummary = data.describe()\nsummary.to_csv('summary.csv')\nprint('Summary saved to summary.csv')"
}
```

---

## 与执行命令的对比

| 特性 | ptc | [执行命令](/docs/zh-CN/tools/exec/) |
|------|-----|--------------------------------|
| 适用场景 | 先生成脚本再执行 | 运行已有命令 |
| 输入 | Python 脚本内容 | 命令字符串 |
| 推荐使用 | 图表、数据处理、程序化 | 编译、测试、脚本 |
| 文件编辑 | ❌ 不适合 | ❌ 不适合 |

---

## 适用场景

✅ **适合使用 ptc**：
- 生成图表（matplotlib、plotly）
- 数据清洗和转换（pandas）
- 输出结构化中间文件
- 先形成脚本再继续执行链路

❌ **不适合使用 ptc**：
- 运行已有命令 → 用 `执行命令`
- 编辑仓库文件 → 用 `写入文件` 或 `应用补丁`
- 执行系统命令串 → 用 `执行命令`

---

## 执行流程

1. 系统把脚本写到工作区下的临时目录（如 `ptc_temp`）
2. 调用 Python 执行脚本
3. 返回执行结果和输出

---

## 注意事项

1. **文件名限制**：
   - 只接受简单文件名，不允许路径段
   - 没写扩展名时，按 Python 脚本处理
   - 非 Python 扩展会被拒绝

2. **用途定位**：
   - 重点不是编辑仓库源码
   - 而是快速产生一次性的程序化产物

3. **文件编辑不用 ptc**：
   - 真正要改仓库文件，优先用文件工具

---

## 延伸阅读

- [执行命令](/docs/zh-CN/tools/exec/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
