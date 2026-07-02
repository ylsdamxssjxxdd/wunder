# PptxGenJS Fallback Workflow

仅当当前环境没有可用 PPT 生成工具，或 `ppt_write` / `ppt_refine` 无法满足用户明确要求时，才读取并执行本文档。默认不要绕过 PPT 生成工具。

## Contents

- [离线依赖自检](#离线依赖自检)
- [渲染引擎选择](#渲染引擎选择)
- [质量基线](#质量基线)
- [自动质量检查](#自动质量检查)
- [从零创建工作流](#从零创建工作流)
- [页面输出格式](#页面输出格式)
- [主题对象契约](#主题对象契约)
- [页码徽标](#页码徽标)

## 离线依赖自检

在开始脚本生成前先做自检；失败时直接降级，不要安装依赖。

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

## 渲染引擎选择

- 没有 PPT 生成工具时优先 `PptxGenJS`，仅在当前执行环境确认为不可用时才降级 `python-pptx`。
- 自检要在同一执行环境内完成（同一个 sandbox 会话），避免“宿主机不可用、容器可用”的误判。
- `python-pptx` 作为兜底时，必须使用卡片化布局和形状系统，不可退化为“纯标题 + 纯项目符号”。

## 质量基线

- 页数建议：`8-12` 页（封面 1 页 + 目录/章节/内容/总结）。
- 每个非封面页至少包含：`1` 个结构底板 + `2` 个以上内容容器 + 右下页码徽标。
- 版式变化要求：同一 deck 至少使用 `3` 种不同布局（例如目录宫格、时间轴、对比卡片、总结页）。
- 文本密度要求：单页正文不超过 `6` 个长段落，优先卡片短句与分层标题。
- 视觉密度基线（经验阈值）：平均每页 `>= 10` 个 shape，平均每页 `>= 6` 个文本框；低于阈值视为“信息图形化不足”。
- 形状类型必须使用 `pres.shapes.*` 枚举常量（如 `pres.shapes.OVAL`），不要传入不受支持的字符串（例如 `"oval"`），否则可能生成 PowerPoint 不可修复文件。
- 导出前必须执行自动质量检查，不通过则返工页面布局。

## 自动质量检查

```bash
python {{SKILL_ROOT}}/scripts/pptx_quality_check.py \
  pptx/output/presentation.pptx \
  --min-slides 8 \
  --min-avg-shapes 10 \
  --min-avg-text-boxes 6 \
  --check-page-badge
```

若检查失败，优先重做“低密度页面”（通常是目录页和信息列表页），再重新导出与复检。

## 从零创建工作流

### 步骤 1：需求研究

先明确用户需求：主题、受众、用途、语气、内容深度。信息来源仅限用户输入、当前会话附件与工作区内文件；不要把联网检索作为默认前置步骤。

### 步骤 2：选择配色与字体

从 [设计系统配色](design-system.md#color-palette-reference) 选择与主题匹配的色板；从 [字体参考](design-system.md#font-reference) 选择中英文字体组合。

### 步骤 3：选择视觉风格

从 [样式配方](design-system.md#style-recipes) 中选择风格（Sharp / Soft / Rounded / Pill），保证整体一致。

### 步骤 4：规划页面大纲

将每一页归类为 [5 种页面类型](slide-types.md) 之一，并规划该页内容与布局。务必保证视觉变化，不要每页都套同一版式。

### 步骤 5：生成每页 JS 文件

在工作区建议目录 `pptx/slides/` 中每页创建一个 JS 文件。每个文件必须导出同步函数 `createSlide(pres, theme)`。遵循本文 [页面输出格式](#页面输出格式) 以及 [slide-types.md](slide-types.md) 的具体类型约束。

可并行生成（若支持子代理，一次最多并行 5 页）时，统一约束：

1. 文件命名：`pptx/slides/slide-01.js`、`pptx/slides/slide-02.js`...
2. 图片目录：`pptx/slides/imgs/`
3. 最终输出：`pptx/output/`
4. 画布尺寸：10" x 5.625"（`LAYOUT_16x9`）
5. 字体：中文 `Microsoft YaHei`，英文 `Arial`（或批准替代）
6. 颜色：6 位十六进制且不带 `#`
7. 必须遵守 [主题对象契约](#主题对象契约)
8. API 以 [pptxgenjs.md](pptxgenjs.md) 为准

### 步骤 6：合并编译为最终 PPTX

创建 `pptx/slides/compile.js` 汇总所有页面模块：

```javascript
// pptx/slides/compile.js
const path = require('path');
const pptxgen = require('pptxgenjs');
const pres = new pptxgen();
pres.layout = 'LAYOUT_16x9';

const theme = {
  primary: "22223b",
  secondary: "4a4e69",
  accent: "9a8c98",
  light: "c9ada7",
  bg: "f2e9e4"
};

for (let i = 1; i <= 12; i++) {
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

### 步骤 7：质量检查

详见 [QA 流程](pitfalls.md#qa-process)。

### 输出目录结构

```text
pptx/
├── slides/
│   ├── slide-01.js
│   ├── slide-02.js
│   ├── ...
│   ├── imgs/
│   └── compile.js
└── output/
    └── presentation.pptx
```

## 页面输出格式

每一页都应是一个可独立运行的 JS 文件：

```javascript
// slide-01.js
const pptxgen = require("pptxgenjs");

const slideConfig = {
  type: 'cover',
  index: 1,
  title: '演示标题'
};

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

## 主题对象契约

`compile.js` 传入 `theme` 时，必须使用以下固定键名：

| 键名 | 含义 | 示例 |
|-----|------|------|
| `theme.primary` | 最深色，标题/重点文本 | `"22223b"` |
| `theme.secondary` | 次深色，正文强调 | `"4a4e69"` |
| `theme.accent` | 中间强调色 | `"9a8c98"` |
| `theme.light` | 浅强调色 | `"c9ada7"` |
| `theme.bg` | 背景色 | `"f2e9e4"` |

不要使用其它键名（如 `background`、`text`、`muted`、`darkest`、`lightest`）。

## 页码徽标

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
