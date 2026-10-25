# 舵机 · 终端端 UI 设计稿

以 `crates/wunder-cli` 当前实现为事实基线的终端视觉规范。用于改稿、重构对照、实现验收和新人理解组件边界，不是营销稿。

- 画布：终端单元格即像素；分页统一 1600px 宽，深色终端块只是参考渲染，不代表固定终端主题
- 唯一外层框架：转写区 / 弹层带 / 欢迎横幅带 / 活动行 / 输入区
- 语义由命名色 + 字重修饰承载：Cyan/Green/Red/Yellow/Magenta + BOLD/DIM/REVERSED/UNDERLINED/CROSSED_OUT/ITALIC
- 双语界面（zh/en）；NO_COLOR 时保留修饰与布局，具体降级以实际函数为准
- 更新日期：2026-10-25

## 文件

| 文件 | 内容与状态 |
| --- | --- |
| `00-整体总览.svg` | 启动实帧、真实欢迎像素画、五段功能栈、布局规则与调用形态 |
| `01-基础规范.svg` | 命名色、Rgb 面板、字重修饰、NO_COLOR、显示列宽与终端兼容 |
| `02-布局与底栏.svg` | 布局约束、运行帧、活动行、输入内部行预算与底栏收缩 |
| `03-会话时间线.svg` | 条目样式、Markdown、流式、代码差异、Ctrl+T 全展开、选区与搜索 |
| `04-工具与计划.svg` | 工具行、补丁、计划标题与步骤、轨迹页签与状态符号 |
| `05-输入与弹层.svg` | 输入区、弹层带、居中选择、任务中心、运行动效与品牌标记 |

统一画廊位于上级目录 [`../index.html`](../index.html)；本目录旧 `index.html` 仅作兼容跳转。

## 查看与修改

1. 使用统一画廊直接拖动大 SVG，支持离线、缩放、键盘与原始文件打开。
2. 改稿先核对 `theme.rs` 的真实常量、各视图的布局与状态投影，不在稿中“顺手”改掉语义。
3. 只画终端可达的效果，不使用图片资产模拟控件；`00` 的欢迎图是从当前生成数据产生的内嵌像素图，不是额外 UI 图标体系。
4. 改完运行 `python scripts/sync_art.py`，发布统一画廊、文档站与蛹内嵌资产（后者不涉及舵机稿）。

## 核心变量

### 命名色语义（随终端主题映射）

| Token | 终端色 | 修饰 | 用途 |
| --- | --- | --- | --- |
| `accent` | Cyan | BOLD | 强调、问询/授权、选中 |
| `success` | Green | — | 一般成功状态 |
| `danger` | Red | BOLD | 错误 |
| `warning` | Yellow | BOLD | 警告计数 |
| `brand` | Magenta | — | 品牌语义；启动头部另有 Cyan+BOLD 的局部实现 |
| `link` | Cyan | BOLD + UNDERLINED | 链接 |
| `secondary` | 无色相 | DIM | 次级文本全局基线 |

### 面板真彩色

| Token | 值 | 用途 |
| --- | --- | --- |
| `surface` | Rgb(24,36,48) | 输入区与居中选择面板表面、转写选区底 |
| `selected` | Rgb(26,42,54) | 普通弹层/模态选中行 |
| `picker-selected` | Rgb(40,74,106) | 居中选择列表的整行选中填充 |
| `diff-hunk` | Rgb(22,30,50) | 差异行头底（前景 Cyan） |
| `diff-added` | Rgb(18,48,31) / Rgb(157,230,188) | 新增行 |
| `diff-deleted` | Rgb(58,24,24) / Rgb(255,182,182) | 删除行 |
| `breath` | Rgb(92,188,255) → Rgb(232,248,255) | 运行呼吸点 |

16/256 色终端的近似映射由终端完成，稿中不虚构另一组近似色值。

### 布局与状态

- 输入区外层固定 5 行、活动行 1 行；弹层高 = min(项数,7)+1；转写地板 8 行（有弹层 6 行）。
- 主布局固定开销 14 行；横幅高度 = min(请求值, 屏高 −（弹层+1+5+8）)。空间不足先压缩或取消横幅。
- 输入区内部另行分配：无通知时草稿 3 + 间隙 1 + 底栏 1；云错误、警告与间隙同时出现时，草稿可被压到 1 行并内部滚动，不能写成“永远草稿 3 行”。
- 底栏先缩减描述，再跳过低优先级项；宽度允许时优先保留后续键名，极端情况使用回退提示，不写绝对的“入口永不消失”。
- 居中选择最大 72 列、列表最多 8 行；普通弹层与居中选择的选中底色不同。
- 计划标题为“已更新计划 · 完成 n/m”；折叠最多 3 步，排序为进行中 → 待执行 → 已完成；完成步骤使用 DIM+CROSSED_OUT，不是普通绿色成功项。
- 恢复线程、审批、问询使用居中选择；快捷键/警告使用信息类居中面板；任务中心是独立整屏视图。

### 动效

| 时长 | 用途 |
| --- | --- |
| 48 ms | 运行帧（呼吸点） |
| 1400 ms | 彩色呼吸周期（cyan→白） |
| 560 ms | NO_COLOR 下 Cyan+BOLD / DIM 交替 |
| 90 ms | 审批/问询/提示待回应唤醒 |
| 10 s | 空闲保活帧 |

欢迎像素画只绘制一次，不运行常驻动画；复制反馈按到期时间安排清除帧。

## 事实来源

- `crates/wunder-cli/tui/theme.rs`（命名色、Rgb、修饰与降级）
- `crates/wunder-cli/tui/ui/layout.rs`、`composer.rs`、`popup.rs`、`centered_picker.rs`、`modals.rs`、`transcript.rs`
- `crates/wunder-cli/tui/app.rs`（banner / activity / footer / picker 投影）
- `crates/wunder-cli/tui/command_center.rs`、`tui/trajectory/view.rs`、`tui/app/plan_panel.rs`、`tui/app/patch_log.rs`
- `crates/wunder-cli/tui/activity_indicator.rs`、`markdown_render.rs`、`markdown_stream.rs`、`highlight.rs`
- `crates/wunder-cli/empty_state_animation.rs`、`welcome_logo_art.rs`（位于 crate 根部，不在 `tui/` 子目录）

## 实机差异与待决项

1. 呼吸点颜色、启动头部 Cyan+BOLD、居中选择的专用选中色都是局部实现，不能只凭单一 token 推断所有表面。
2. 真彩色在 16/256 色终端的回退由终端完成；若未来改由代码做降级映射，再补对应规范。
3. 示例线程名、路径与模型名均为通用示例，不对应真实业务数据。
4. 欢迎横幅来自生成数据；修改横幅先改生成源，再重新生成并校验内嵌图，不能手工画占位九宫格代替。

## 校验

- 分页均为独立 SVG；欢迎像素数据以 Base64 内嵌，单张拷走也能完整显示。
- 所有分页使用 Edge 按原始宽度渲染，并检查终端网格、文字溢出与选中状态。
- 运行 `python scripts/sync_art.py` 后，舵机稿发布到 `web/docs/art/servo/`，统一画廊同时包含两套稿。
- 若在 Inkscape/Figma 中编辑，保留命名分组；避免将文字转曲，除非确定不再修改文案。
