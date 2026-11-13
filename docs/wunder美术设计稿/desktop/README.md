# wunder 桌面版 · 界面设计稿

以 `crates/wunder-desktop/frontend/` 当前实现为事实基线的桌面视觉规范（wunder 桌面版 = 本地 AI 工作台）。用于改稿、重构对照、实现验收和新人理解组件边界，不是营销稿。

- 窗口：preferred 960 × 780；最小 880 × 700；标题栏 28
- 侧栏：默认 240，可拖 170–420；阅读列 ≤1000，内边距 24；输入带 `8 + 100 + 6 + 36 + 10 = 160`
- 窗口标题由线程聚合状态决定：`wunder 桌面版 · 空闲` / `运行中 N 线程` / `问询 N`
- 默认强调色 `#F58A24`（橙汁）；设置页只提供四张主题卡：橙汁 / 薄荷 / 森林 / 蜜蜂
- 窗口四边各 1px `window-frame` 暖灰 `#B9B2A7`，不是 `border-strong`
- 分页画布 1600 px 宽；`00` 直接嵌当前版本的客户区实机快照（1200 × 836，显示缩放 1.375×），其余分页的演示区按 1462 × 866 放大绘制，**尺寸以文字标注和 `design_tokens.slint` 为准**
- 图标资产：`frontend/assets/icons/` 共 132 个文件，本套稿只列 47 个高频实例
- 截图资产：`desktop/assets/ui-overview.png` 与桌面版帮助截图同源，替换时两处一起换；`assets/eva01-mark.png` 取自 `frontend/assets/`。替换后运行 `python scripts/sync_art.py`，`embed_art_assets.py` 会重新内嵌 data URI。替换后运行 `python scripts/sync_art.py`，`embed_art_assets.py` 会重新内嵌 data URI
- 截图资产：`desktop/assets/ui-overview.png` 与桌面版帮助截图同源，替换时两处一起换；`assets/eva01-mark.png` 取自 `frontend/assets/`。替换后运行 `python scripts/sync_art.py`，`embed_art_assets.py` 会重新内嵌 data URI。替换后运行 `python scripts/sync_art.py`，`embed_art_assets.py` 会重新内嵌 data URI
- 更新日期：2026-11-13

## 文件与功能分区

| 文件 | 内容与状态 |
| --- | --- |
| `00-整体总览.svg` | 当前版本的客户区实机快照、五类功能分区；新线程、完成回合与输入空闲的区别 |
| `01-基础规范.svg` | 中性色、强调色族、语义色、字阶、间距、圆角与窗口度量 |
| `02-图标系统.svg` | 47 个真实生产图标，按导航、会话、输入、状态分类；尺寸与着色 |
| `03-外壳与侧栏.svg` | 标题栏、窗口按钮、工作区/线程行、目录、分隔柄；行交互态、生命周期与多选 |
| `04-会话时间线.svg` | 完整回合、用户气泡、过程、正文、页脚、Markdown、空态与状态胶囊 |
| `05-过程与工具.svg` | 过程折叠、工具行、展开详情、工具组、子智能体、补丁差异；运行/完成/失败 |
| `06-计划与问询.svg` | 计划胶囊与步骤、目标横幅、问询面板；选项、其他、无偏好与分页 |
| `07-输入区.svg` | 编辑卡、动作行、本机状态格、附件/分支、队列、加号菜单、模型选择；发送/停止组合状态 |
| `08-设置与弹窗.svg` | 真实设置壳、11 类导航、控件、Confirm/Modal/Info/Toast；校验与 busy |
| `09-交互状态与动效.svg` | 真实 Action/FormField/ThemeCheck/Switch 状态、键盘、动效、无障碍与断点 |

统一画廊位于上级目录 [`../index.html`](../index.html)。

## 本版删除与更正

上一版把已移除的形态名、已删除的功能页和已下线的图标继续画在稿里。本版按当前实现清理：

1. 形态命名统一为 **wunder 桌面版**；稿面 kicker、`<title>`、`aria-label`、窗口标题、侧栏设备标签同步。
2. 设置导航删掉「提示词」分类：实际为 **11 类**（通用 / 运行时 / 账号 / 模型 / 工具 / 智能体 / 伙伴 / 定时 / 记忆 / 渠道 / 归档），顶部另有搜索框。
3. 加号菜单由 6 项改为 **4 项 + 1 条件项**：命令、预设问题、语音输入、截图；「打开命令行」仅桌面端可拉起 CLI 时出现。**审批模式已从输入区与加号菜单移除**，当前桌面版没有模式选择入口；侧栏设备行只用 `shield.svg` 表示「等待审批」状态。
4. 「附件」没有独立生产图标：附件由拖入与粘贴产生，稿中该格改为真实存在的**分支状态**（`fa-code-branch`）。
5. 线程轨迹图标是 `fa-route.svg`，不是 `timeline.svg`；展开/折叠是 `chevron-down.svg`，不是 `disclosure`。
6. 本机状态读数行是 **4 个液位格 + 2 个计数格**：CPU / 内存 / 网络 / 磁盘，后台任务与子智能体各一格；旧稿的「三格、网络与磁盘不占格」已作废。
7. 窗口度量更正：preferred 960 × 780、最小 880 × 700、动作行 36、输入带 160。
8. 主题卡名称更正为「橙汁 / 薄荷 / 森林 / 蜜蜂」；旧稿的「森林绿 / 蜂蜜黄 / 羊皮纸橙」不再使用。
9. 删除编辑版画廊 `editorial/`：它按 v0.3.4 重做、与本套稿重复且未接入文档站，继续保留只会产出第二份过期规范。

## 核心布局与控件事实

- 标题栏 28 px；窗口按钮 3 × 38 × 28；拖拽区 `width − 114`。
- 侧栏默认 240 px，可拖 170–420；分隔柄命中区 6 px，静止无可见条，悬停 2×32、拖动 2×40 accent。
- 侧栏顶部是设备条（本地 / 云端 + `wunder 桌面版` / `wunder 云端版`），不是品牌名；审批状态用 `shield.svg` 标在设备条上。
- 侧栏行节奏（`SidebarMetrics`）：线程行 32、组行 36 + 4px 组间气口、两侧留白 12、行内边距 8、圆角 8；线程状态点列缩进 28、标题接到工作区标题列（48）；字号标题 13 / 次级 11，图标底 20 / 字形 13；行内动作按钮 20px 触面 + 4px 间距。
- 线程悬停动作真实顺序为**重命名 / 轨迹 / 归档**（`fa-pen-to-square` / `fa-route` / `box-archive`）；工作区操作是另一组，不混用。
- 输入带：`8 + 100 + 6 + 36 + 10 = 160`；内容宽 = 输入带 − 左右各 14。附件 +36，分支 +22，队列按可见行数增加。
- 动作行左侧是加号（24）与本机状态格（4 × 52 + 2 × 76），右侧是模型触发器与 28 px 发送方块；编辑器 14px SimSun。
- 发送门禁：`(草稿 || 附件 || 已答问询) && !busy`。生成中没有可发送内容时才显示停止；有内容时仍显示发送并进入队列；`stopping` 时停止停用。
- 录音/转写胶囊位于编辑卡右上，22px 高；不改变输入卡高度。
- 计划胶囊 26 px，展开卡 ≤560 px、最大高 360；目标横幅按钮顺序为**编辑 / 暂停或恢复 / 清除**。
- 问询面板没有提交/跳过按钮，答案由 Composer 发送；空草稿但当前页有答案时发送可用。
- 模型弹层 300×340；加号菜单 220，命令与预设问题带级联 268；审批模式不在菜单内。
- 设置壳 `min(1080, w−72) × min(720, h−56)`，头部 44，导航 260，导航项 48 高 + 4 间距、单行条目；导航与内容都可滚动，顶部搜索无结果时显示专用空态。
- 主题选择使用四张 110px 高预览卡；常规页没有“界面密度、开机启动、全局保存”这些控件。
- 弹窗：Confirm 400×160；Modal 420×220；Info 宽 `min(520, 窗宽−72)`、卡高 = 正文高 + 150，正文超过 360px 封顶滚动。

## 状态与动效

- 控件悬停按组件使用 `hover` 或 `accent-soft`，不位移、不放大；输入与 Action 聚焦为 1px accent，无外发光。
- 展开/折叠是本地视图状态；计划步骤、目标阶段、问询答案和线程状态来自运行时投影，不能混为同一状态源。
- 工具过程节点：运行 = 280° 弧，完成 = 细环勾，失败 = 细环叉；展开/折叠不改变执行状态。
- 流式输出按 16–33ms 合并，只更新活动尾块；历史限载与虚拟化，事件缓冲有界。
- 侧栏六点旋转周期 1.2s；模型与 Dock 为 1.48s；勾选淡入 90ms；录音脉冲 550ms；复制反馈 1.6s。
- 系统 `Switch` 使用 Slint 自身蓝色开启态，不跟随 `Theme.accent`。

## 实机差异与待决项

1. 实心按钮的 1px accent 焦点在 accent 底上对比较弱；普通停用按钮不统一降透明度；ThemeCheck 没有独立焦点描边。
2. 目标横幅粉色 `#FDF1F6` / `#EFC6D7`、状态胶囊色和部分组件颜色是局部值，不能反推为全局 token。
3. Windows 默认 Microsoft YaHei UI；其他平台需确认字体回退后的字宽与截断。
4. 示例文件名、线程名与模型名均为通用示例，不对应真实业务数据。
5. 除 `00` 的实机快照外，演示区按 1462 宽放大绘制，真实窗口在 960 宽下会把阅读列压到 720；判断断点行为请看 `09` 的响应式格，不要量演示区像素。

## 事实来源

- `crates/wunder-desktop/frontend/ui/design_tokens.slint`、`theme.slint`、`main.slint`
- `crates/wunder-desktop/frontend/ui/window_chrome.slint`、`sidebar.slint`、`workspace_files.slint`、`drag_row.slint`
- `crates/wunder-desktop/frontend/ui/timeline.slint`、`tool_process.slint`、`status_icon.slint`、`thread_trajectory.slint`
- `crates/wunder-desktop/frontend/ui/composer.slint`、`host_hud.slint`、`textedit-base.slint`、`text_field.slint`
- `crates/wunder-desktop/frontend/ui/settings_page.slint`、`components.slint`、`patch_diff_card.slint`、`workflow_detail.slint`
- `crates/wunder-desktop/frontend/ui/markdown-rich.slint`、`activity_card.slint`、`runtime_status.slint`
- `crates/wunder-desktop/frontend/src/turn_stats.rs`、`tool_icons.rs`、`demo.rs` 与当前 Slint 软件渲染帧

## 校验

- 所有分页使用 Edge 按原始 1600px 宽渲染，检查文字溢出、卡片遮挡、真实图标与状态。
- 源文件保留 `assets/app-icon.png` 外链；发布时 `build_docs_site.py` 自动内嵌为 data URI。
- 桌面版帮助不内嵌本稿预览图，使用真实界面截图 `crates/wunder-desktop/frontend/assets/help/art/ui-overview.png`；本稿只服务设计对照与文档站画廊。
- 若在 Inkscape/Figma 中编辑，保留 `defs` 与命名分组；避免将文字转曲，除非确定不再修改文案。
