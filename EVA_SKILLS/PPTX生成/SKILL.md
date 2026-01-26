---
name: PPTX生成
description: "使用 PptxGenJS 通过直接绘制形状与文本来创建或更新 .pptx。适用于需要 JavaScript 脚本化生成 PPTX、希望可编程构建幻灯片、或明确要求避免 html2pptx/Playwright 的场景。"
---

# PptxGenJS PPTX 生成

## 概览

通过 PptxGenJS 直接在幻灯片上放置形状与文字，使用 Node.js 脚本生成 PPTX。

## 工作流

1. 先确定版式与风格（配色、字体、间距、页尺寸）。
2. 复制技能包内的 `scripts/pptxgenjs-starter.js`（注意文件名）到工作目录并重命名为 `build.js`。
3. 先用 `读取文件` 查看 `build.js`，仅修改 `OUTPUT_FILE`、`SLIDES` 与样式常量，避免改动 import/require 行。
   - 修改 `SLIDES` 时请使用 `// CONTENT_START` 与 `// CONTENT_END` 之间的完整块作为 `old_string`，不要凭印象替换。
   - 若用户指定文件名，优先改 `OUTPUT_FILE`，避免额外重命名步骤。
   - 使用 `替换文本` 时必须检查 `replaced > 0`；若为 0，请先 `读取文件` 再改用 `编辑文件` 精确修改。
4. 运行 `node build.js` 生成 PPTX，并用 `列出文件` 确认输出文件存在。
5. 打开 PPTX 检查并微调间距、对齐与字号。

## 脚本约定

- `OUTPUT_FILE` 控制输出文件名（默认 `output.pptx`）。
- 脚本直接写入 `OUTPUT_FILE`，请在 `build.js` 所在目录运行 `node build.js`。
- `SLIDES` 为数组，每一项包含 `title` 与 `bullets`（字符串数组）。
- 字体与颜色通过常量行配置：`TITLE_FONT`、`BODY_FONT`、`TITLE_COLOR`、`BODY_COLOR`、`ACCENT_COLOR`。
- 修改样式时优先用 `替换文本` 精确替换上述常量行，避免按行号编辑。

## 失败处理

- 若执行 `node build.js` 提示 `pptxgen is not defined`，请恢复首行 `const pptxgen = require('pptxgenjs');`，不要改用其他 require 路径。
- 建议仅替换 `CONTENT_START/END` 之间的块，避免影响脚本结构。

## 布局与格式规则

- 颜色使用不带 `#` 的十六进制（示例：`FF6F61`）。
- 默认设置 `pptx.layout = 'LAYOUT_16x9'`，除非明确需要自定义尺寸。
- 16:9 标准尺寸为 10 x 5.625 英寸，注意四边留白。
- 标题使用黑体（脚本默认 SimHei）。
- 绘制顺序：先背景，再内容块，再文本。
- 文本框保持在边距内，避免裁切。

## 资源

- `scripts/pptxgenjs-starter.js`：含辅助函数与示例的起步脚本。
- `references/pptxgenjs-rules.md`：颜色、单位与布局的速记规则。
