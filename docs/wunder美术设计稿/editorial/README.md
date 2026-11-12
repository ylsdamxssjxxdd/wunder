# wunder 美术设计稿 · 编辑版

本目录是 `docs/wunder美术设计稿/` 的**重做版本**，按 `diagram-design` 技能（v2.6）的方法论重新设计，
以当前工作树（v0.3.4）的实现为事实基线。

产物为 **10 张自包含 HTML**（内嵌 CSS + inline SVG，仅外链 Google Fonts），统一 1280 × 780
（正文 1280 × 720 + 60px 图例带）。

- 总索引：`index.html`
- 旧稿（像素规范版 16 张 SVG）保留在 `../蛹/`、`../舵机/`，作为历史对照，未删除、未覆盖。

---

## 1. 为什么重做

旧稿是**像素级界面规范**：1600px 画布、逐区域标注尺寸与色值。它作为实现手册有效，但作为「设计稿」有两个问题：

1. **讲不动结构**。窗口里能看到的东西被逐个框出来，但读不出「为什么这样切」「哪一段是弹性的」「什么条件下变成另一个状态」。
2. **密度反噬**。一张图塞十几块标注，读者要来回对照才能拼出因果；改稿讨论时无法指认「问题在第几步」。

新版改为**编辑型图表**：每张图只回答一个问题，用固定的一套形状/连接线/排版规则承载结构、状态与约束。
不做界面模拟（技能明确反对逐像素复刻 pattern）。

---

## 2. 清单

### 蛹 · 桌面端（6 张）

| 文件 | 类型 | 讲什么 |
|---|---|---|
| `00-window-anatomy.html` | Architecture | 窗口三列解剖：外框、标题栏、侧栏、阅读列、输入带的固定与弹性关系 |
| `01-sidebar-tree.html` | Tree | 工作区 → 分组 → 线程 / 文件树的四层层级与吸顶分组 |
| `02-thread-lifecycle.html` | State machine | 六态八转移；等待不占忙碌位，失败可重试 |
| `03-tool-process.html` | Architecture | 事件 → 行条目 → 分页 → 视口锚定，以及 v0.3.4 的三条滚动约束 |
| `04-composer-gate.html` | Flowchart | 发送门禁的判定顺序：内容在前、忙碌在后 |
| `05-token-stack.html` | Layer stack | 基色板 → 语义别名 → 组件 token → 版式 → 间距，含对比度契约 |

### 舵机 · 终端（4 张）

| 文件 | 类型 | 讲什么 |
|---|---|---|
| `10-tui-layout.html` | Layer stack | 五段纵向布局：哪段弹性、哪段永远不动 |
| `11-tui-colors.html` | Key / 矩阵 | 五个命名色 + 默认，三种修饰；色不作唯一信号 |
| `12-tui-input-budget.html` | Layer stack | 输入区五行分工与转写地板 8 / 6 行差 |
| `13-tui-popup.html` | Flowchart | 触发、筛选、空态、高亮移动、确认；弹层高度与居中选择 |

---

## 3. 品牌 skin 定制（有意偏离，已记录）

技能默认 token 是它的通用配色；本项目已有成熟视觉体系，因此**定制 skin** 而非照搬默认值。
映射关系：

| 技能语义角色 | 技能默认 | 本套取值 | 来源 |
|---|---|---|---|
| paper | `#f5f5f5` | `#FBFAF8` | `design_tokens.slint` panel |
| paper-2 | — | `#F3F1ED` | timeline-bubble / selected 派生 |
| ink | `#2d3142` | `#1F2329` | text |
| muted | `#4f5d75` | `#5C6470` | text-secondary / muted 之间 |
| soft | `#5a6580` | `#636B76` | muted 提亮 |
| **accent** | `#bf4520` | **`#B4531A`** | 品牌 `#F58A24` 的加深版 |
| link | `#2e5aa8` | `#2E5AA8` | 同值 |
| rule | ink @ 0.12 | `rgba(31,35,41,0.12)` | border 派生 |

**accent 为什么不用品牌橙 `#F58A24`**：该色在 `#FBFAF8` 底上对比度 2.46:1，作文字不满足 4.5:1。
`#B4531A` 为同色相加深版，白底 4.8:1，满足 WCAG AA。图内已把这条契约写成可见说明
（`05-token-stack.html` 底部条），属**有意偏离并在图上自解释**，不算违规。

字体角色保持技能规定：title 用 Instrument Serif，正文节点名 Geist 600 12px，
sublabel 用 Geist Mono 9px，中文一律回落 Noto Sans SC / PingFang SC。
中文不作 eyebrow / arrow-label（技能要求该两处若用中文需改 12px sans 500 无字距）。

---

## 4. 遵循的硬规则

- 复杂度预算：每图 ≤9 节点、≤12 箭头、≤2 coral 元素、≤2 注释；超出即拆分。
- 连接线：仅正交折线（圆角 r=8），禁对角线，禁重叠与共线，同边扇出间距 ≥12px，不穿过非端点盒子。
- 无障碍：`role="img"` + `aria-labelledby`，`<title>` 为 `<svg>` 第一个子元素，ID 命名 `<slug>-title` / `<slug>-desc`。
- 尺寸：`viewBox 1280 780`，`min-width` 等于 viewBox 宽，SVG 置于局部 `overflow-x:auto` 容器，`@media print` 释放两者。
- 无阴影、无渐变、无 3D；纯色填充 + 1px hairline。

---

## 5. 校验

```bash
python "D:/proj/参考/diagram-design-main/skills/diagram-design/scripts/self_check.py" docs/wunder美术设计稿/editorial/00-window-anatomy.html
```

对 10 张逐个执行，预期输出 `OK`。几何关系人工核对（技能目录下 `verify-geometry.py` 本地不存在，
仅 `self_check.py` 可用）。

---

## 6. 位图（PNG）

每张图都有同名 `.png`：**2560 × 1560**，即 1280×780 的 `scale=2`。用于直接预览、贴进文档、发群，不需要别人再打开 HTML。

- 标准路径：`python config/skills/图纸设计/scripts/export_png.py <src.html> <out.png> 2`（技能默认 `scale=2`）。
- 本机当前**未安装 playwright**，标准脚本会直接 `ModuleNotFoundError`；按技能规定不装依赖、不改环境。本次走等价降级：抽出 `<svg>` 本体 → 包进 1280×780 无边框页面 → headless Edge `--force-device-scale-factor=2` 截图，脚本在 `.tmp/svg2png.py`。输出像素与标准路径一致，只出图纸本体，编辑外壳照旧丢弃。
- 环境补上 playwright 后，优先回到技能自带脚本，不要长期依赖 Edge 降级。
- 改了任一张 HTML，就要重导对应 PNG，否则两者会不一致。

## 7. 与发布链的关系

现有 `scripts/sync_art.py` 面向 SVG 与文档站，对本目录的 HTML **尚未接入**。
如需把编辑版发布到文档站，需另加一步「HTML 直出 + iframe 缩略图」，当前未做。