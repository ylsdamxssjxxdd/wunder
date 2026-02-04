---
name: PPTX生成
description: "使用 PptxGenJS 脚本化生成中文 PPTX（避免 HTML/Playwright），适用于创建或更新中文演示文稿/幻灯片/Deck，并希望基于多套模板快速产出一致版式的场景。"
---

# PptxGenJS 中文PPT生成

## 概述
- 用 PptxGenJS 直接绘制形状与文本生成 .pptx
- 内置 4 套模板（general / modern / standard / swift）
- 适合脚本化、可复现、可批量产出

## 模板清单与包含版式
- `scripts/pptxgenjs-starter.js`（general）
  - Intro / Agenda / Features / Metrics / Quote&Closing
- `scripts/pptxgenjs-starter-modern.js`（modern）
  - Intro / Problem / Solution / Market Size / Traction / Closing
- `scripts/pptxgenjs-starter-standard.js`（standard）
  - Intro / Outline / Bullet+Image / Metrics / Closing
- `scripts/pptxgenjs-starter-swift.js`（swift）
  - Intro / Contents / Feature Cards / Timeline / Metrics

## 推荐流程（更明确版）
1. **需求收集**：主题、受众、目标、语言、页数、交付时间、图片/图标是否提供。
2. **输出大纲**：每页 1 行（标题 + 目的），确认结构再开始绘制。
3. **选模板**：根据风格选择脚本（general/modern/standard/swift）。
4. **复制脚本**：把所选脚本复制为 `build.js`。
5. **只改 DATA**：优先修改 `DATA` 区域，不动辅助函数。
6. **生成 PPTX**：`node build.js` → `output.pptx`。
7. **文本 QA**：`python -m markitdown output.pptx` 检查错字/缺页/顺序。
8. **视觉 QA**：有 LibreOffice/Poppler 时转换图片逐页检查。
9. **修复复检**：修改 `build.js` 后重新生成并复检。

## 编辑规则
- **只改 DATA**：模板脚本已封装布局，优先改 `DATA`。
- **输出文件名**：修改 `OUTPUT_FILE`，不要改 `writeFile` 结构。
- **图片未提供**：保持 `null`，模板会生成占位图块。
- **不要写 #**：颜色必须是 6 位十六进制且不带 `#`。
- **不要复用 options**：PptxGenJS 会原地修改对象。
- **列表必须 bullet**：使用 `bullet: true` + `breakLine: true`。
- **文本对齐**：对齐到形状边缘时 `margin: 0`。

## 版式扩展方式（推荐）
- 复制现有 slide 代码块，改标题与数据。
- 需要新布局时：
  1) 新建一个 `addXxx` 辅助函数
  2) 在 `DATA` 中加入对应结构
  3) 新增一个 slide block

## 坐标与尺寸
- 默认 `LAYOUT_16x9`，尺寸 10 x 5.625 英寸
- 需要 4:3：改 `pptx.layout = "LAYOUT_4x3"` 并手动调整布局

## QA
- 文本：`python -m markitdown output.pptx`
- 视觉（有 LibreOffice + Poppler）：
  - `soffice --headless --convert-to pdf output.pptx`
  - `pdftoppm -jpeg -r 150 output.pdf slide`