---
名称: pptx
描述: "基于模板快速生成与编辑 PPTX。适合需要批量出稿、统一风格或按大纲生成演示文稿的场景。"
---

# PPTX 模板化制作

## 适用范围
- 需要快速产出演示文稿，并保持统一视觉风格
- 有明确大纲或结构化内容（标题/要点/图片）
- 希望复用模板而不是从零排版

## 核心工具（Dockerfile.rust 已内置）
- `python-pptx`：从模板添加幻灯片并填充占位符
- `PyYAML`：解析大纲 YAML
- `markitdown`：导出 PPTX 文本做校对（可选）
- `libreoffice`/`poppler-utils`：转 PDF/图片做质检（可选）

## 快速流程

1. **选择模板**
   - 列出模板：`python scripts/list_templates.py`
   - ????????`templates/manifest.yaml`
   - ?????????????-??????? / ??????-????????
   - ?????????????`python scripts/patch_placeholders.py`

2. **查看模板布局与占位符**
   - `python scripts/inspect_template.py templates/教学设计模板-浅.pptx`
   - 记录需要用到的 `layout` 与 `placeholder idx`

3. **编写大纲**
   - 使用 YAML/JSON（示例：`examples/outline.yaml`、`examples/psych-education.yaml`）
   - 推荐：先写大纲再填占位符

4. **生成 PPTX**
   - `python scripts/build_deck.py --template templates/教学设计模板-浅.pptx --outline examples/outline.yaml --output output.pptx`

5. **快速校对（可选）**
   - 导出文本：`python -m markitdown output.pptx > output.md`

## 大纲格式说明（简化版）

```yaml
meta:
  template: ../templates/教学设计模板-浅.pptx
  title: 项目名称
  slide_ratio: "16:9"
  theme:
    background: "#F7F9FB"
    accent: "#5B8FF9"
    accent_light: "#E8F0FF"
    accent_bar_height_in: 0.35
    title_block_height_in: 1.4

slides:
  - layout: 0
    title: 标题页标题
    subtitle: 标题页副标题

  - layout: 1
    title: 议程
    bullets:
      - 要点 1
      - 要点 2

  - layout: 1
    title: 正文页
    placeholders:
      1:
        bullets:
          - text: 主项
            level: 0
          - text: 子项
            level: 1
```

说明：
- `layout` 支持 **索引** 或 **名称**（建议先用 `inspect_template.py` 确认）
- `placeholders` 用占位符 `idx` 精确映射
- `title/subtitle/body/bullets` 为快捷字段（未写 `placeholders` 时生效）
- `notes` 可写演讲者备注（字符串）
- `meta.slide_ratio` 默认 `16:9`，可选 `4:3`/`16:10`，或设为 `template` 保持模板比例
- `meta.theme` 可选：`background`/`accent`/`accent_light`/`accent_bar_height_in`/`title_block_height_in`

## 模板更新规则
- 新模板必须记录来源与许可证到 `templates/manifest.yaml`
- 许可证原文需放入 `templates/licenses/`
- 如果模板来自外部仓库，请保留上游链接

> 默认优先使用模板化生成流程，避免直接操作 OOXML。
