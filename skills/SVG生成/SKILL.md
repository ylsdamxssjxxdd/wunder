---
name: SVG生成
description: Generate SVG images using native SVG syntax for PPT/论文/海报等场景的矢量图需求，覆盖流程图、示意图、图表或封面插图，并提供模板与脚本辅助。
---

# SVG 图片生成

## 概要
使用本技能直接编写 SVG（可缩放矢量图形）生成高质量图表、流程图、示意图与封面插图，面向 PPT/论文等需要可编辑与高清输出的场景。

## 快速流程
1. 确定画布尺寸与比例（如 16:9 或 4:3）。
2. 使用原生 SVG 语法绘制图形与文本。
3. （可选）运行脚本模板快速生成并微调。
4. 在 PPT/论文中直接插入或导出 PNG/PDF 使用。

## 画布尺寸建议
- 16:9：`1600x900`、`1920x1080`
- 4:3：`1200x900`
- 正方图：`1000x1000`

## SVG 编写要点
- 使用 `viewBox` 保持缩放一致。
- 用 `text` + `text-anchor` + `dominant-baseline` 对齐文本。
- 使用内联样式，避免依赖外部 CSS。
- 优先 PPT/Word 兼容性：避免复杂滤镜堆叠，尽量使用基础形状。
- 选择可用中文字体，例如 `"Microsoft YaHei", "PingFang SC", "Noto Sans CJK SC", sans-serif`。

## 示例与模板
- 示例流程图：`examples/sample-diagram.svg`
- 示例图表：`examples/sample-chart.svg`
- 脚本模板：`scripts/svg_starter.py`

### 简易模板（可直接复制）
```svg
<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="900" viewBox="0 0 1600 900">
  <rect width="1600" height="900" fill="#f8fafc" />
  <text x="80" y="120" font-size="48" font-family="Microsoft YaHei, sans-serif" fill="#0f172a">
    标题示例
  </text>
  <rect x="80" y="200" width="480" height="180" rx="16" fill="#e0f2fe" stroke="#38bdf8" />
  <text x="320" y="300" font-size="28" text-anchor="middle" dominant-baseline="middle" fill="#0f172a">
    模块 A
  </text>
</svg>
```

## 脚本模板用法
```bash
python scripts/svg_starter.py --output out.svg --title "技术文档标准化流程" --steps "采集" "加工" "发布"
```

## 输出建议
- 若需导出 PNG，可使用 Inkscape 或其他矢量工具导出并保持清晰度。
- 插入 PPT 时建议保留 SVG 以便后续编辑。

## 常见问题
- 文本对齐不准：优先使用 `text-anchor="middle"` 和 `dominant-baseline="middle"`。
- 字体替换：确保目标环境安装中文字体，或改用内置字体。
