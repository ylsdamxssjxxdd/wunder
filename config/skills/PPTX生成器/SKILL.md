---
name: PPTX生成器
description: "生成、编辑与读取 PowerPoint 演示文稿。若当前环境提供 ppt_write、ppt_refine、ppt_read、ppt_template_read 等 PPT 生成工具，必须优先使用工具生成和精修；无工具时再基于 PptxGenJS 或 python-pptx 从零创建，也可通过 XML 工作流编辑现有 PPTX，或用 markitdown 提取文本。触发词：PPT、PPTX、PowerPoint、演示文稿、幻灯片、Deck。"
---

# PPTX 生成与编辑

## 概述

本技能覆盖 PowerPoint 全流程任务：优先通过可用 PPT 生成工具创建、读取、精修演示文稿；无工具时读取/分析已有演示文稿、通过 XML 改写模板型 PPTX，或基于 PptxGenJS 从零生成演示文稿。技能内置设计系统（配色、字体、样式配方）和各页面类型的实现规范。

## 快速参考

| 任务 | 推荐方式 |
|------|----------|
| 有 `ppt_write` 等 PPT 生成工具 | 优先走 [PPT 生成工具优先流程](#ppt-生成工具优先流程) |
| 读取/分析内容 | `python -m markitdown presentation.pptx` |
| 编辑模板或现有文稿 | 见 [编辑演示文稿](references/editing.md) |
| 无 PPT 生成工具时从零创建 | 见下方 [从零创建工作流](#从零创建工作流) |

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
| [pptxgenjs-fallback.md](references/pptxgenjs-fallback.md) | 没有 PPT 生成工具时的 PptxGenJS / python-pptx 脚本兜底流程 |

---

## PPT 生成工具优先流程（强约束）

如果当前环境、MCP、函数调用或工具列表中存在 `ppt_write`、`ppt_refine`、`ppt_read`、`ppt_template_read`、`ppt_delete` 之一，默认使用这些工具完成 PPT 任务；不要先手写 PptxGenJS / python-pptx 脚本。只有确认没有可用 PPT 工具，或工具无法满足用户明确要求时，才进入后文脚本工作流。

推荐流程：

1. **读取模板能力**：先调用 `ppt_template_read` 空参数，查看可用内置模板和真实 PPTX 母版模板包。需要统一字体和母版复用时优先选择母版模板包，例如 `black_times_default`、`top_title_section` 或用户自定义 `config/ppt_templates/<template_id>/`；用户要求封面页、顶部全宽大标题、顶部左侧一级标题、下方正文/图片内容区时，优先使用 `top_title_section`。使用 `top_title_section` 时，第一页写 `type=cover` 并只表达封面标题/副标题；第二页开始的 `title` 是顶部大标题，`subtitle` 是左上一级标题，`body`、`bullet`、`item` 和 `image` 放入下方内容区。
2. **规划页面结构**：根据用户资料规划封面、目录、章节、内容、图文、数据、对比、时间线、总结/结尾等页面。第一页按封面处理，最后一页按结尾处理，中间页按 `type`、`layout` 或 `template_slide_id` 选择版式。
3. **生成初稿**：调用 `ppt_write`，优先传结构化 XML：

```xml
<slides>
  <slide>
    <type>cover</type>
    <title>标题</title>
    <subtitle>副标题</subtitle>
    <prompt>封面页设计要求</prompt>
  </slide>
  <slide>
    <type>content_image</type>
    <title>图文页标题</title>
    <body>正文要点</body>
    <image src="/workspaces/.../image.png" />
    <prompt>图文页设计要求</prompt>
  </slide>
</slides>
```

4. **写入工作区**：`output_path` 优先使用当前工作区下的导出路径，例如 `/workspaces/<workspace_id>/pptx/output/presentation.pptx`。不要把最终产物写到技能目录或临时目录。
5. **读取核验**：生成后调用 `ppt_read` 检查页数、标题、页面摘要和 `slide_id`，确认结构完整。
6. **迭代精修**：需要局部调整时调用 `ppt_refine`，传入目标 `slide_id` 和修改后的结构化内容；需要整体换风格时传新的 `template_id`。删除页面用 `ppt_delete`。
7. **图片处理**：图片必须先位于本地或工作区路径，再通过 XML `<image src="..." />` 或 JSON `images` 数组传给工具；不要在 PPT 内容里直接嵌远程 URL 或 base64 大块数据。
8. **交付用户**：默认只交付最终 `.pptx` 路径；只有用户要求时再提供 PDF、预览图、生成过程或修改清单。

---

## Wunder 沙盒适配（强约束）

- 本技能运行在 wunder 的 Docker 沙盒环境，默认按“离线可执行”设计。
- **禁止**在执行阶段联网安装依赖（不要运行 `pip install` / `npm install` / `apt-get`）。
- 只使用镜像内预装能力；缺失时改走降级方案，不要尝试联网补齐。
- 产物必须落到当前智能体工作区，不写到 `temp_dir`、技能目录或其他系统目录。
- 路径优先使用工作区相对路径。
- 若工具必须传绝对路径，先在当前会话执行 `pwd` 获取实际工作区根（通常为 `/workspaces/<workspace_id>`）后再拼接；不要手写 `/workspaces/{user_id}`。

### 输出路径约定（工作区内）

- 用户提供的原始资料优先放在 `pptx/input/`。
- 最终产物优先放在 `pptx/output/`。
- 脚本兜底方案的 `slides/`、`imgs/`、编译脚本等细节只在 [pptxgenjs-fallback.md](references/pptxgenjs-fallback.md) 中说明。

---

## 读取现有 PPT 内容

```bash
# 文本提取
python -m markitdown presentation.pptx
```

---

## 从零创建工作流

**适用场景：确认没有可用 PPT 生成工具，且没有模板或参考演示文稿，需从头生成。**

此时读取 [pptxgenjs-fallback.md](references/pptxgenjs-fallback.md)，再按其中的 PptxGenJS / python-pptx 脚本兜底流程执行。不要把脚本兜底流程作为默认路径。

---

## 依赖

- 本技能按 wunder 沙盒“预装依赖”运行，不在执行阶段安装依赖。
- 已预装（见 Dockerfile）：`markitdown[docx,pptx,xlsx]`、`python-pptx`、`pptxgenjs`、`react-icons`、`react`、`react-dom`、`sharp`。
- 仅在没有可用 PPT 生成工具、必须走脚本兜底方案时，才读取 [pptxgenjs-fallback.md](references/pptxgenjs-fallback.md) 并执行离线依赖自检。
