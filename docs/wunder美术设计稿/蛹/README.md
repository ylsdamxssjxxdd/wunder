# 蛹 · 桌面端 UI 设计稿

以 `frontend-slint/` 当前实现为事实基线的桌面端视觉规范。用于改稿、重构对照、实现验收和新人理解组件边界，不是营销稿。

- 总览窗口：1462 × 866 逻辑像素；preferred：1180 × 740；最小：1024 × 700
- 分页画布：1600 px 宽；产品 UI 按 1× 逻辑像素绘制，稿面注释独立排版
- 默认强调色：`#F58A24`；中性色不随强调色切换
- 阅读列：≤1000 px，时间线与输入带共用列宽，内边距 24 px
- 应用图标：`assets/app-icon.png`；`bee.svg` 仅保留为渠道/协作辅助标记
- 更新日期：2026-10-25

## 文件与功能分区

| 文件 | 内容与状态 |
| --- | --- |
| `00-整体总览.svg` | 真实窗口、五类功能分区；新线程、完成回合与输入空闲的区别 |
| `01-基础规范.svg` | 中性色、强调色族、语义色、字阶、间距、圆角与布局基线 |
| `02-图标系统.svg` | 30 个真实生产图标，按导航、会话、输入、状态分类；尺寸与着色 |
| `03-外壳与侧栏.svg` | 标题栏、窗口按钮、工作区/线程行、目录、分隔柄；行交互态、生命周期与多选 |
| `04-会话时间线.svg` | 完整回合、用户气泡、过程、正文、页脚、Markdown、空态与状态胶囊 |
| `05-过程与工具.svg` | 过程折叠、工具行、展开详情、工具组、子智能体、补丁差异；运行/完成/失败 |
| `06-计划与问询.svg` | 计划胶囊与步骤、目标横幅、问询面板；选项、其他、无偏好与分页 |
| `07-输入区.svg` | 编辑卡、动作行、附件/分支、队列、加号菜单、模型选择；发送/停止组合状态 |
| `08-设置与弹窗.svg` | 真实设置壳、主题卡、控制件、Confirm/Modal/Info/Toast；校验与 busy |
| `09-交互状态与动效.svg` | 真实 Action/FormField/ThemeCheck/Switch 状态、键盘、动效、无障碍与断点 |

统一画廊位于上级目录 [`../index.html`](../index.html)；本目录旧 `index.html` 仅作兼容跳转。

## 查看与修改

1. 使用统一画廊直接拖动大 SVG；支持适宽、适窗、100%、缩放、键盘与离线打开。
2. 改稿优先复用每页 `<defs>` 的图形与 `01` 的 token，不在页面里发明新的灰色、字号或控件状态。
3. 需要落地到代码时，仍以 `frontend-slint/ui/design_tokens.slint` 为唯一变量来源；本稿不是运行时代码。
4. 改完运行 `python scripts/sync_art.py`，刷新 Web 文档、统一画廊与蛹内嵌预览图。
5. 生产图标本就混用 Font Awesome 实心与仓库线性 SVG，不得再写成“统一 1.7px 线性重绘”。新增或替换图标必须核对真实资产、viewBox、光学尺寸与 colorize 行为。

## 核心布局与控件事实

- 标题栏 28 px；窗口按钮 3 × 38 × 28；拖拽区 `width − 114`。
- 侧栏默认 240 px，可拖 170–420；分隔柄命中区 6 px，静止无可见条，悬停 2×32、拖动 2×40 accent。
- 线程悬停动作真实顺序为**重命名 / 轨迹 / 归档**；工作区操作是另一组，不混用。
- 输入带：`8 + 100 + 6 + 30 + 10 = 154`；内容宽 = 输入带 − 左右各 14 = 972（列宽 1000 时）。附件 +36，分支 +22，队列按可见行数增加。
- 编辑器使用 14px SimSun；发送/停止位于动作行右下，不位于输入卡内。
- 发送门禁：`(草稿 || 附件 || 已答问询) && !busy`。生成中没有可发送内容时才显示停止；有内容时仍显示发送并进入队列；`stopping` 时停止停用。
- 录音/转写胶囊位于编辑卡右上，22px 高；不改变输入卡高度。
- 计划胶囊 26 px，展开卡 ≤560 px、最大高 360；展开卡只有说明与步骤，没有额外套标题或计数。
- 目标横幅按钮顺序为**编辑 / 暂停或恢复 / 清除**；`elapsed` 字段当前不渲染。
- 问询面板没有提交/跳过按钮，答案由 Composer 发送；空草稿但当前页有答案时发送可用。
- 模型弹层 300×340；加号菜单 220；级联 268。
- 设置壳 `min(1080, w−72) × min(720, h−56)`，头部 44，导航 260，导航项 60 高 + 4 间距；导航与内容都可滚动。
- 主题选择使用四张 110px 高预览卡，不是小色块；常规页没有“界面密度、开机启动、全局保存”这些控件。
- 弹窗：Confirm 400×160；Modal 420×220；Info 720×620。

## 状态与动效

- 控件悬停按组件使用 `hover` 或 `accent-soft`，不位移、不放大；输入与 Action 聚焦为 1px accent，无外发光。
- 展开/折叠是本地视图状态；计划步骤、目标阶段、问询答案和线程状态来自运行时投影，不能混为同一状态源。
- 工具过程节点：运行 = 280° 弧，完成 = 细环勾，失败 = 细环叉；展开/折叠不改变执行状态。
- 流式输出按 16–33ms 合并，只更新活动尾块；历史限载与虚拟化，事件缓冲有界。
- 侧栏六点旋转周期 1.2s；模型与 Dock 为 1.48s；勾选淡入 90ms；录音脉冲 550ms；复制反馈 1.6s。
- 系统 `Switch` 使用 Slint 自身蓝色开启态，不跟随 `Theme.accent`；普通停用按钮也不保证统一灰化，按真实实现展示。

## 实机差异与待决项

以下只记录当前实现，不在稿里提前改成理想状态：

1. 实心按钮的 1px accent 焦点在 accent 底上对比较弱；普通停用按钮不统一降透明度；ThemeCheck 没有独立焦点描边。
2. 目标横幅粉色 `#FDF1F6` / `#EFC6D7`、状态胶囊色和部分组件颜色是局部值，不能反推为全局 token。
3. Windows 默认 Microsoft YaHei UI；其他平台需确认字体回退后的字宽与截断。
4. 示例文件名、线程名与模型名均为通用示例，不对应真实业务数据。

## 事实来源

- `frontend-slint/ui/design_tokens.slint`、`theme.slint`、`main.slint`
- `frontend-slint/ui/window_chrome.slint`、`sidebar.slint`、`workspace_files.slint`
- `frontend-slint/ui/timeline.slint`、`tool_process.slint`、`status_icon.slint`
- `frontend-slint/ui/composer.slint`、`textedit-base.slint`、`settings_page.slint`
- `frontend-slint/ui/components.slint`、`patch_diff_card.slint`、`workflow_detail.slint`
- `frontend-slint/ui/markdown.slint`、`activity_card.slint`、`runtime_status.slint`
- `frontend-slint/src/turn_stats.rs`、`tool_icons.rs`、`demo.rs` 与当前 Slint 软件渲染帧

## 校验

- 所有分页使用 Edge 按原始 1600px 宽渲染，检查文字溢出、卡片遮挡、真实图标与状态。
- 源文件保留 `assets/app-icon.png` 外链；发布时 `build_docs_site.py` 自动内嵌为 data URI，避免 `<img>` 加载时丢失嵌套资源。
- 蛹帮助手册使用 `render_art_previews.py` 生成的 1200px PNG，输出只在 `frontend-slint/assets/help/art`；不要手工编辑生成图。
- 若在 Inkscape/Figma 中编辑，保留 `defs` 与命名分组；避免将文字转曲，除非确定不再修改文案。
