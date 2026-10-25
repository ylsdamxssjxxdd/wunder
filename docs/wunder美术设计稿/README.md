# 蛹 · 桌面端 UI 设计稿

以 `frontend-slint/` 当前实现为事实基线的桌面端视觉规范。用于改稿、重构对照、实现验收和新人理解组件边界，不是营销稿。

- 基准窗口：1440 × 900（逻辑像素）；`00` 总览按参考截图 175% DPI 折算为 1462 × 866 做 1:1 像素对照
- 最小窗口：1024 × 700；preferred：1180 × 740
- 画布宽度：1600 px（分页纸张），UI 本体按 1× 绘制
- 默认强调色：`#F58A24`（橙汁）；中性色不跟随强调色
- 阅读列：≤ 1000 px，左右各 24 px
- 应用图标：`assets/app-icon.png`（蛹，16 px 主用法）；`bee.svg` 保留为渠道 / 协作辅助标记
- 更新日期：2026-10-25

## 文件

| 文件 | 内容 |
| --- | --- |
| `00-整体总览.svg` | 完整窗口、结构索引与区块定位 |
| `01-基础规范.svg` | 中性色、强调色族、语义色、字阶、间距、圆角、布局基线 |
| `02-图标系统.svg` | 48 个统一化图标、尺寸与着色规则 |
| `03-外壳与侧栏.svg` | 标题栏、窗口按钮、侧栏行、状态点、工作目录、设置入口 |
| `04-会话时间线.svg` | 阅读列、用户气泡、过程折叠、输出卡、Markdown、回合页脚、空态 |
| `05-过程与工具.svg` | 过程条、思考条目、工具行状态、工具组、子智能体、差异卡、工作流详情 |
| `06-计划与问询.svg` | 计划胶囊/卡片、目标横幅、问询面板、选项状态 |
| `07-输入区.svg` | 输入卡、发送/停止、附件、录音、队列、加号菜单、模型选择 |
| `08-设置与弹窗.svg` | 设置壳、控制件、确认/模态/信息弹窗、Toast、菜单、Tooltip |
| `09-交互状态与动效.svg` | 按钮/行/输入矩阵、动效、键盘、无障碍、断点、反馈层级 |
| `index.html` | 画廊索引，可直接打开或打印 |

## 快速使用

1. 直接双击 `index.html` 浏览全部页面；或单独打开任一 SVG。源稿的蛹图标是外链，仅在以 `<img>` 方式加载时可能不显示，用 SVG 编辑器或构建后的站点查看即可。
2. 改稿时优先复用每页 `<defs>` 中的组件和 `01` 的 token，不要在页面里发明新的灰色或字号。
3. 需要落地到代码时，仍以 `frontend-slint/ui/design_tokens.slint` 为唯一变量来源；本稿是视觉说明，不是运行时代码。
4. 改完 SVG 后运行 `python scripts/sync_art.py`，同时刷新 Web 文档与蛹内嵌的预览图。

## 核心变量

### 中性色

| Token | 值 | 用途 |
| --- | --- | --- |
| `text` | `#1F2329` | 主文本 |
| `text-secondary` | `#3D3D3D` | 次级文本 |
| `muted` | `#8A8F99` | 辅助、占位、图标 |
| `panel` | `#FBFAF8` | 会话底 |
| `surface` | `#FFFFFF` | 卡片、输入卡 |
| `border` | `#E8E6E3` | 常规描边 |
| `border-strong` | `#D8D5D0` | 控件描边 |
| `hover` | `#F6F5F3` | 行悬停、侧栏 |
| `selected` | `#F1EFEC` | 选中行 |
| `sidebar` | `#F6F5F3` | 侧栏底 |
| `timeline-bubble` | `#F3F2F0` | 用户气泡 |
| `timeline-line` | `#E2DFDA` | 过程连接线 |
| `timeline-entry-soft` | `#9BA0A8` | 过程次级结果 |
| `activity-off` | `#E9E8E5` | 活跃格关闭 |

### 强调色（橙汁）

| Token | 值 |
| --- | --- |
| `accent` | `#F58A24` |
| `accent-deep` | `#E07A12` |
| `accent-secondary` | `#FFC98A` |
| `accent-soft` | `#FDF0DE` |
| `mine` | `#FDF0DE` |

可切换色族：`hula-green #16AD8D`、`forest #2F7A5C`、`honey #C08A2A`、`minimal #8F9AAD`、`tech-blue #38BDF8`。设置页提供前四项，`minimal` / `tech-blue` 为保留项。

### 语义色

| Token | 值 |
| --- | --- |
| `success` / `success-bg` | `#1A8A5C` / `#EAF7F0` |
| `warning` / `warning-bg` | `#C98A2B` / `#FBEAE5` |
| `danger` / `danger-bg` | `#D04A43` / `#FDECEC` |
| `info` | `#3B82F6` |
| `thought` | `#3E9C7E` |

### 字阶与圆角

- 字号：10 / 11 / 12 / 13 / 14 / 16 / 18 / 24 px；正文 14，行高 1.5。
- 字体：Microsoft YaHei UI → Microsoft YaHei → PingFang SC → Noto Sans SC。
- 等宽：Consolas 11–13 px。
- 圆角：6 / 8 / 10 / 12 / 14 / 16 / 胶囊（高度一半）。
- 间距：4 / 8 / 12 / 16 / 20 / 24 px。

## 关键布局

- 标题栏 28 px；窗口按钮 3 × 38 × 28；拖拽区 `width − 114`。
- 侧栏 240 px（可拖 170–420）；分隔柄命中区 6 px，静止无可见条、悬停 2×32、拖动 2×40 accent，不绘制整条色带。轨迹详情右侧柄同构，命中区 8 px、静止保留 1 px 线、强调色用 `brand`。
- 会话与输入共用阅读列：`min(width, 1000)`，左右 24 px。
- 输入区：`8 + 100 + 6 + 30 + 10 = 154`；附件 +36，分支 +22，队列按行增长。停止方块 10.5×10.5（12 px 图像盒 contain），发送箭头保持 16 px 盒。
- 计划胶囊 26 px；计划卡 ≤560 px，最大高 360。步骤环 18 px / 1.6 px 描边；完成勾约 9×6，暂停双条合计 5×6.2（条宽 1.7，间隔 1.8），均居中。
- 问询面板：外高 10 + 卡 + 12；卡宽随阅读列。
- 模型弹层 300 × 340；+ 菜单 220；级联 268。
- 设置壳 `min(1080, w−72) × min(720, h−56)`，导航 260。
- 弹窗：Confirm 400×160；Modal 420×220；Info 720×620。

## 交互与状态

- 悬停只换底：`hover` / `accent-soft`；不位移、不放大。
- 聚焦：输入与描边按钮 1 px accent；无外发光。
- 时间线与工具行的展开/折叠瞬时生效；长列表不做布局动画。
- 流式输出按帧合并（16–33 ms），只更新活动尾块；历史虚拟列表。
- 工具状态：运行 = 280° 弧；完成 = 勾；失败 = 叉；待 = 时钟。
- 发送门禁：`(草稿 || 附件 || 已答问询) && !busy`；生成中无草稿时显示停止。
- 问询面板没有提交/跳过按钮；答案由 Composer 发送键提交。
- 计划卡、目标横幅、问询面板都由运行时投影；前端不推导状态。

## 动效

| 时长 | 用途 |
| --- | --- |
| 16 ms | 流式跟随（连续 2 帧） |
| 33 ms | 文本/工具尾随、分页 |
| 90 ms | 勾选淡入 |
| 110 ms | 队列插入线、拖拽落点 |
| 150 ms | 图标着色 |
| 550 ms | 录音脉冲（若启用） |
| 1.2 s | 状态胶囊呼吸 |
| 1.48 s | 转圈 |
| 1.6 s | 代码复制反馈 |

## 事实来源

- `frontend-slint/ui/design_tokens.slint`
- `frontend-slint/ui/theme.slint`
- `frontend-slint/ui/main.slint`
- `frontend-slint/ui/sidebar.slint`
- `frontend-slint/ui/window_chrome.slint`
- `frontend-slint/ui/timeline.slint`
- `frontend-slint/ui/tool_process.slint`
- `frontend-slint/ui/patch_diff_card.slint`
- `frontend-slint/ui/composer.slint`
- `frontend-slint/ui/settings_page.slint`
- `frontend-slint/ui/i18n.slint`
- `frontend-slint/ui/runtime_status.slint`
- `frontend-slint/ui/status_icon.slint`
- `frontend-slint/src/turn_stats.rs`
- `frontend-slint/src/tool_icons.rs`

## 与代码的差异、待决项

设计稿以“当前实现 + 可维护性”双重目标绘制，以下条目需要重构时拍板：

1. **实心按钮焦点环**：代码为 1 px accent，叠在 accent 底上几乎不可见。设计稿用 2 px 深色环示意；建议重构时统一为 2 px 或外偏移焦点环。
2. **问询面板没有提交/跳过按钮**：这是当前实现，不是遗漏。答案通过 Composer 发送；设计稿在提示文案中明确。
3. **计划卡没有标题头**：只有进度与关闭；`计划` 文案在胶囊上。
4. **目标横幅粉色**：`#FDF1F6` / `#EFC6D7` 是局部颜色，未收敛为全局 token；建议重构时并入 warning/goal 语义。
5. **图标风格混用**：生产资产混用 Font Awesome 实心与线性图标；本稿统一为 24×24、1.7 px 线性重绘，替换时需逐项核对语义与光学尺寸。
6. **`elapsed` 未渲染**：目标横幅的数据字段存在但当前 UI 不显示，设计稿保留说明。
7. **字体回退**：Windows 默认 Microsoft YaHei UI；Linux/其他平台需确认回退后的字宽与截断策略。
8. **示例文本**：文件名、线程名、模型名均为通用示例，落地时替换为真实数据；不要将示例写进测试或文档。

## 校验

- 所有分页已用 Edge headless 渲染检查，画布为 1600 px 宽，无文字溢出或卡片遮挡（最终版本）。
- 源文件保留可读的 `assets/app-icon.png` 外链；`build_docs_site.py` 复制到 `web/docs/art` 时自动内嵌为 data URI，避免 SVG 以 `<img>` 加载时外部资源被浏览器拦截。
- Web 文档直接使用 SVG；蛹帮助手册使用 `scripts/render_art_previews.py` 生成的 1200 px PNG（唯一一份，在 `frontend-slint/assets/help/art`）。改完图统一运行 `python scripts/sync_art.py`。
- 图标页的 `<symbol>` 默认 `fill="none" stroke="currentColor"`，实心状态符单独覆盖。
- 若在 Inkscape/Figma 中编辑，保留 `defs` 与命名分组；避免将文字转曲，除非确定不再改文案。
