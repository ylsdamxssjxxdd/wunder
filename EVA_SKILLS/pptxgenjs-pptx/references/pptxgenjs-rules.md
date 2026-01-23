# PptxGenJS 速记规则

## 核心约束
- 颜色使用不带 # 的十六进制（示例：FF6F61）。
- 单位为英寸，16:9 标准尺寸是 10 x 5.625。
- 添加幻灯片前先设置 pptx.layout。
- 绘制顺序：先背景，再内容形状，再文本。

## 布局建议
- 四边留至少 0.4 英寸边距。
- 对重复元素使用辅助函数（如页眉、卡片）。
- 文本尽量短行，避免裁切。

## 字体建议
- 使用安全字体：Arial, Helvetica, Times New Roman, Georgia, Courier New, Verdana, Tahoma, Trebuchet MS, Impact。
- 标题用加粗，正文字号控制在 12-16 pt。
