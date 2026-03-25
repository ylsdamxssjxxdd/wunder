---
name: PPTX生成器
description: "生成、编辑与读取 PowerPoint 演示文稿。可基于 PptxGenJS 从零创建（封面、目录、内容、章节分隔、总结页），也可通过 XML 工作流编辑现有 PPTX，或用 markitdown 提取文本。触发词：PPT、PPTX、PowerPoint、演示文稿、幻灯片、Deck。"
---

# PPTX 生成与编辑

## 概述

本技能覆盖 PowerPoint 全流程任务：读取/分析已有演示文稿、通过 XML 改写模板型 PPTX、以及基于 PptxGenJS 从零生成演示文稿。技能内置设计系统（配色、字体、样式配方）和各页面类型的实现规范。

## 快速参考

| 任务 | 推荐方式 |
|------|----------|
| 读取/分析内容 | `python -m markitdown presentation.pptx` |
| 编辑模板或现有文稿 | 见 [编辑演示文稿](references/editing.md) |
| 从零创建 | 见下方 [从零创建工作流](#从零创建工作流) |

| 项目 | 约定值 |
|------|--------|
| **尺寸** | 10" x 5.625"（`LAYOUT_16x9`） |
| **颜色格式** | 6 位十六进制，不带 `#`（如 `"FF0000"`） |
| **英文字体** | Arial（默认）或已批准替代字体 |
| **中文字体** | Microsoft YaHei |
| **页码徽标位置** | x: `9.3"`，y: `5.1"` |
| **主题键** | `primary`、`secondary`、`accent`、`light`、`bg` |
| **基础形状** | `RECTANGLE`、`OVAL`、`LINE`、`ROUNDED_RECTANGLE` |
| **图表类型** | `BAR`、`LINE`、`PIE`、`DOUGHNUT`、`SCATTER`、`BUBBLE`、`RADAR` |

## 参考文件

| 文件 | 内容 |
|------|------|
| [slide-types.md](references/slide-types.md) | 5 种页面类型（封面/目录/章节分隔/内容/总结）及扩展布局模式 |
| [design-system.md](references/design-system.md) | 配色、字体、样式配方（Sharp/Soft/Rounded/Pill）、排版与间距规范 |
| [editing.md](references/editing.md) | 基于模板编辑流程、XML 修改方法、格式规则与常见问题 |
| [pitfalls.md](references/pitfalls.md) | QA 流程、常见错误、PptxGenJS 关键陷阱 |
| [pptxgenjs.md](references/pptxgenjs.md) | PptxGenJS API 速查 |

---

## Wunder 沙盒适配（强约束）

- 本技能运行在 wunder 的 Docker 沙盒环境，默认按“离线可执行”设计。
- **禁止**在执行阶段联网安装依赖（不要运行 `pip install` / `npm install` / `apt-get`）。
- 只使用镜像内预装能力；缺失时改走降级方案，不要尝试联网补齐。
- 产物必须落到当前智能体工作区，不写到 `temp_dir`、技能目录或其他系统目录。
- 路径优先使用工作区相对路径。
- 若工具必须传绝对路径，先在当前会话执行 `pwd` 获取实际工作区根（通常为 `/workspaces/<workspace_id>`）后再拼接；不要手写 `/workspaces/{user_id}`。

### 推荐目录约定（工作区内）

```text
pptx/
├── input/        # 用户提供的原始资料
├── slides/       # 逐页脚本与素材
│   ├── imgs/
│   └── compile.js
└── output/       # 最终产物（pptx/pdf/中间文件）
```

---

## 离线依赖自检（必做）

在开始生成/编辑前先做自检；失败时直接降级，不要安装依赖。

```bash
# 1) Python 依赖：markitdown + python-pptx
python3 - <<'PY'
import importlib.util
need = ["markitdown", "pptx"]
missing = [m for m in need if importlib.util.find_spec(m) is None]
print("missing:", missing)
raise SystemExit(1 if missing else 0)
PY

# 2) Node 依赖：优先使用镜像中的全局包
export NODE_PATH="$(npm root -g):${NODE_PATH}"
node -e "require('pptxgenjs'); console.log('pptxgenjs ok')"
```

若 `pptxgenjs` 不可用，则改用 `python-pptx` 方案生成（保持同样的目录与产物路径约定）。

---

## 读取现有 PPT 内容

```bash
# 文本提取
python -m markitdown presentation.pptx
```

---

## 从零创建工作流

**适用场景：没有模板或参考演示文稿，需从头生成。**

### 步骤 1：需求研究

先明确用户需求：主题、受众、用途、语气、内容深度。信息来源仅限用户输入、当前会话附件与工作区内文件；不要把联网检索作为默认前置步骤。

### 步骤 2：选择配色与字体

从 [设计系统配色](references/design-system.md#color-palette-reference) 选择与主题匹配的色板；从 [字体参考](references/design-system.md#font-reference) 选择中英文字体组合。

### 步骤 3：选择视觉风格

从 [样式配方](references/design-system.md#style-recipes) 中选择风格（Sharp / Soft / Rounded / Pill），保证整体一致。

### 步骤 4：规划页面大纲

将**每一页**归类为 [5 种页面类型](references/slide-types.md) 之一，并规划该页内容与布局。务必保证视觉变化，不要每页都套同一版式。

### 步骤 5：生成每页 JS 文件

在工作区建议目录 `pptx/slides/` 中每页创建一个 JS 文件。每个文件必须导出同步函数 `createSlide(pres, theme)`。遵循本文 [页面输出格式](#页面输出格式) 以及 [slide-types.md](references/slide-types.md) 的具体类型约束。

可并行生成（若支持子代理，一次最多并行 5 页）时，统一约束：
1. 文件命名：`pptx/slides/slide-01.js`、`pptx/slides/slide-02.js`...
2. 图片目录：`pptx/slides/imgs/`
3. 最终输出：`pptx/output/`
4. 画布尺寸：10" x 5.625"（`LAYOUT_16x9`）
5. 字体：中文 `Microsoft YaHei`，英文 `Arial`（或批准替代）
6. 颜色：6 位十六进制且不带 `#`
7. 必须遵守 [主题对象契约](#主题对象契约)
8. API 以 [pptxgenjs.md](references/pptxgenjs.md) 为准

### 步骤 6：合并编译为最终 PPTX

创建 `pptx/slides/compile.js` 汇总所有页面模块：

```javascript
// pptx/slides/compile.js
const path = require('path');
const pptxgen = require('pptxgenjs');
const pres = new pptxgen();
pres.layout = 'LAYOUT_16x9';

const theme = {
  primary: "22223b",    // 深色：背景/标题文本
  secondary: "4a4e69",  // 次级强调
  accent: "9a8c98",     // 高亮色
  light: "c9ada7",      // 浅强调
  bg: "f2e9e4"          // 背景色
};

for (let i = 1; i <= 12; i++) { // 按实际页数调整
  const num = String(i).padStart(2, '0');
  const slideModule = require(`./slide-${num}.js`);
  slideModule.createSlide(pres, theme);
}

const outputPath = path.join(__dirname, '..', 'output', 'presentation.pptx');
pres.writeFile({ fileName: outputPath });
```

执行（离线沙盒建议）：

```bash
export NODE_PATH="$(npm root -g):${NODE_PATH}"
node pptx/slides/compile.js
```

### 步骤 7：质量检查（必做）

详见 [QA 流程](references/pitfalls.md#qa-process)。

### 输出目录结构

```text
pptx/
├── slides/
│   ├── slide-01.js      # 单页模块
│   ├── slide-02.js
│   ├── ...
│   ├── imgs/            # 页面用图
│   └── compile.js
└── output/
    └── presentation.pptx
```

---

## 页面输出格式

每一页都应是一个**可独立运行**的 JS 文件：

```javascript
// slide-01.js
const pptxgen = require("pptxgenjs");

const slideConfig = {
  type: 'cover',
  index: 1,
  title: '演示标题'
};

// 必须是同步函数（不要用 async）
function createSlide(pres, theme) {
  const slide = pres.addSlide();
  slide.background = { color: theme.bg };

  slide.addText(slideConfig.title, {
    x: 0.5, y: 2, w: 9, h: 1.2,
    fontSize: 48, fontFace: "Arial",
    color: theme.primary, bold: true, align: "center"
  });

  return slide;
}

// 独立预览（使用页面专属文件名）
if (require.main === module) {
  const pres = new pptxgen();
  pres.layout = 'LAYOUT_16x9';
  const theme = {
    primary: "22223b",
    secondary: "4a4e69",
    accent: "9a8c98",
    light: "c9ada7",
    bg: "f2e9e4"
  };
  createSlide(pres, theme);
  pres.writeFile({ fileName: "slide-01-preview.pptx" });
}

module.exports = { createSlide, slideConfig };
```

---

## 主题对象契约（强制）

`compile.js` 传入 `theme` 时，必须使用以下固定键名：

| 键名 | 含义 | 示例 |
|-----|------|------|
| `theme.primary` | 最深色，标题/重点文本 | `"22223b"` |
| `theme.secondary` | 次深色，正文强调 | `"4a4e69"` |
| `theme.accent` | 中间强调色 | `"9a8c98"` |
| `theme.light` | 浅强调色 | `"c9ada7"` |
| `theme.bg` | 背景色 | `"f2e9e4"` |

不要使用其它键名（如 `background`、`text`、`muted`、`darkest`、`lightest`）。

---

## 页码徽标（必需）

除封面外，所有页面必须在右下角显示页码徽标：

- 位置：x `9.3"`，y `5.1"`
- 仅显示当前页码（如 `3` 或 `03`），不要显示 `3/12`
- 颜色需与主题协调，避免喧宾夺主

### 圆形徽标（默认）

```javascript
slide.addShape(pres.shapes.OVAL, {
  x: 9.3, y: 5.1, w: 0.4, h: 0.4,
  fill: { color: theme.accent }
});
slide.addText("3", {
  x: 9.3, y: 5.1, w: 0.4, h: 0.4,
  fontSize: 12, fontFace: "Arial",
  color: "FFFFFF", bold: true,
  align: "center", valign: "middle"
});
```

### 胶囊徽标

```javascript
slide.addShape(pres.shapes.ROUNDED_RECTANGLE, {
  x: 9.1, y: 5.15, w: 0.6, h: 0.35,
  fill: { color: theme.accent },
  rectRadius: 0.15
});
slide.addText("03", {
  x: 9.1, y: 5.15, w: 0.6, h: 0.35,
  fontSize: 11, fontFace: "Arial",
  color: "FFFFFF", bold: true,
  align: "center", valign: "middle"
});
```

---

## 依赖

- 本技能按 wunder 沙盒“预装依赖”运行，不在执行阶段安装依赖。
- 已预装（见 Dockerfile）：`markitdown[docx,pptx,xlsx]`、`python-pptx`、`pptxgenjs`、`react-icons`、`react`、`react-dom`、`sharp`。
- 执行前请先完成上文“离线依赖自检（必做）”。
