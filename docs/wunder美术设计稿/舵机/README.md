# 舵机 · 终端端 UI 设计稿

以 `crates/wunder-cli/tui` 当前实现为事实基线的终端视觉规范。用于改稿、重构对照、实现验收和新人理解组件边界，不是营销稿。

- 画布：终端单元格即像素；稿面统一 1600 px 宽，深色终端示意块不代表固定配色
- 唯一框架：五段垂直栈——转写区 / 弹层带 / 欢迎横幅带 / 活动行 / 输入区
- 语义由「命名色 + 字重修饰」承载：Cyan/Green/Red/Yellow/Magenta + BOLD/DIM/REVERSED/UNDERLINED/CROSSED_OUT；面板底色是仅有的真彩色常量
- 双语界面（zh/en）；`NO_COLOR` 时色相落回默认、修饰保留、布局不变
- 更新日期：2026-10-25

## 文件

| 文件 | 内容 |
| --- | --- |
| `00-整体总览.svg` | 终端全帧、五段布局栈、布局规则与调用形态 |
| `01-基础规范.svg` | 语义色、面板真彩色、字重修饰、NO_COLOR 降级、文本宽度、布局基线、终端兼容阶梯 |
| `02-布局与底栏.svg` | 布局栈约束顺序、对话中全帧、欢迎横幅让位、活动行状态表、底栏收缩 |
| `03-会话时间线.svg` | 条目样式表、Markdown 基线、流式规则、代码与差异、Ctrl+T 全展开、选区与搜索 |
| `04-工具与计划.svg` | 工具行与补丁卡、计划面板状态与排序、轨迹视图页签、状态符号表 |
| `05-输入与弹层.svg` | 输入区、弹层带、四种模态、任务中心、运行动效与帧调度、品牌标记 |

## 快速使用

1. 直接打开任一 SVG，或从仓库文档站的设计稿索引页浏览。
2. 改稿时先核对 `theme.rs` 的真实常量：命名色语义、Rgb 面板值、修饰语义都不能在稿中"顺手"改掉；稿与码冲突时先核对实机。
3. 本稿只画终端可达的效果：不使用图片资产、不依赖特定终端主题、不引入稿面之外的色值语义。

## 核心变量

### 命名色语义（随终端主题映射）

| Token | 终端色 | 修饰 | 用途 |
| --- | --- | --- | --- |
| `accent` | Cyan | BOLD | 强调、问询/授权、弹层标题与选中 |
| `success` | Green | — | 成功状态 |
| `danger` | Red | BOLD | 错误 |
| `warning` | Yellow | BOLD | 警告计数 |
| `brand` | Magenta | — | 品牌标记 ⚙ |
| `link` | Cyan | BOLD + UNDERLINED | 链接 |
| `secondary` | 无色相 | DIM | 次级文本全局基线 |

### 面板真彩色（不随终端主题）

| Token | 值 | 用途 |
| --- | --- | --- |
| `surface` | Rgb(24,36,48) | 输入区表面、转写选区底 |
| `selected` | Rgb(26,42,54) | 弹层/模态选中行 |
| `diff-hunk` | Rgb(22,30,50) | 差异行头底（前景 Cyan） |
| `diff-added` | Rgb(18,48,31) / Rgb(157,230,188) | 新增行 |
| `diff-deleted` | Rgb(58,24,24) / Rgb(255,182,182) | 删除行 |
| `breath` | Rgb(92,188,255) → Rgb(232,248,255) | 运行呼吸点 |

### 布局常量

- 输入区 5 行、活动行 1 行；弹层高 = min(项数,7)+1；转写地板 8 行（有弹层 6 行）。
- 主布局固定开销 14 行；横幅带钳制到余量 = 屏高 −（弹层+1+5+8），屏幕越矮先牺牲横幅。

### 动效

| 时长 | 用途 |
| --- | --- |
| 48 ms | 运行帧（呼吸点） |
| 1400 ms | 呼吸周期（cyan→白） |
| 560 ms | NO_COLOR 亮/暗脉冲 |
| 90 ms | 审批/问询/提示待回应唤醒 |
| 10 s | 空闲保活帧 |

## 事实来源

- `crates/wunder-cli/tui/theme.rs`（全部颜色与修饰）
- `crates/wunder-cli/tui/ui/layout.rs` / `composer.rs` / `modals.rs` / `popup.rs` / `transcript.rs`
- `crates/wunder-cli/tui/app.rs`（banner / activity / footer 投影）
- `crates/wunder-cli/tui/command_center.rs`、`tui/trajectory/view.rs`、`tui/app/plan_panel.rs`、`tui/app/patch_log.rs`
- `crates/wunder-cli/tui/activity_indicator.rs`、`tui/empty_state_animation.rs`、`tui/markdown_render.rs`、`tui/highlight.rs`

## 与代码的差异、待决项

1. **呼吸点配色**：`pending_indicator_style` 的呼吸基准色是局部常量，未收敛到 theme.rs；如需换主题色应先收敛再改稿。
2. **差异底色**：真彩色在 16/256 色终端的回退由终端完成，稿中不指定近似值；若未来在代码层做降级映射，需补一档 256 色规范。
3. **示例文案**：线程名、路径、模型名均为通用示例，落地时替换为真实数据；不要将示例写进测试或文档。
4. **欢迎横幅画稿**：稿中像素块为示意；真实横幅来自 `welcome_logo_art.rs` 的生成数据，改横幅先改生成脚本再同步此稿。

## 校验

- 所有分页为纯 SVG（无外链资源），单张拷走可完整显示。
- 运行 `python scripts/sync_art.py` 后，舵机稿会发布到文档站 `web/docs/art/servo/`（使用说明书 art 页引用）；蛹内嵌 PNG 与手册 JSON 不涉及舵机稿。
- 若在 Inkscape/Figma 中编辑，保留命名分组；避免将文字转曲，除非确定不再改文案。
