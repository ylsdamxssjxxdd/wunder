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
3. 按需实现幻灯片，所有尺寸与位置用英寸。
4. 运行 `node build.js` 生成 PPTX，并用 `列出文件` 确认输出文件存在。
5. 打开 PPTX 检查并微调间距、对齐与字号。

## 布局与格式规则

- 颜色使用不带 `#` 的十六进制（示例：`FF6F61`）。
- 默认设置 `pptx.layout = 'LAYOUT_16x9'`，除非明确需要自定义尺寸。
- 标题使用黑体。
- 绘制顺序：先背景，再卡片/块，再文本。
- 文本框保持在边距内，避免裁切。

## 资源

- `scripts/pptxgenjs-starter.js`：含辅助函数与示例的起步脚本。
- `references/pptxgenjs-rules.md`：颜色、单位与布局的速记规则。
