---
title: 形象系统
summary: 形象是挂在智能体上的视觉陪伴层：ZIP 形象包 + 精灵表动画，随运行状态切换动画行，不参与任何执行逻辑。
read_when:
  - 想给智能体配置或导入动画形象
  - 制作自定义形象包，需要知道包格式与精灵表规格
  - 形象不显示、位置丢失或缩放异常
source_docs:
  - crates/wunder-runtime/src/services/companions.rs
  - crates/wunder-runtime/src/api/companions.rs
  - crates/wunder-runtime/src/api/admin/resource_admin.rs
  - frontend-slint/src/companion_sprite.rs
  - frontend-slint/src/pet_window/mod.rs
  - crates/wunder-desktop/native_companion_overlay.rs
  - frontend/src/stores/companions.ts
updated_at: 2026-10-13
---

# 形象系统

## 定位

形象（companion）只是一个**视觉陪伴层**：根据智能体会话的运行状态播放对应动画、偶尔冒消息气泡。它不参与执行、不影响调度，删掉形象不改变任何功能语义。

## 形象包格式

一个形象包是一个 ZIP，恰好包含一份清单和一张精灵表：

| 组成 | 要求 |
| --- | --- |
| `pet.json` | 字段 `id`、`displayName`（兼容别名 `name`）、`description`、`spritesheetPath` |
| 精灵表图片 | 单张图片文件，路径由 `spritesheetPath` 指向 |
| 大小上限 | 整个 ZIP ≤ 24 MB；精灵表单文件 ≤ 18 MB |
| 安全 | 导入时做压缩包内路径穿越防护 |

存储根为 `config/data/companions/global`，或被环境变量 `WUNDER_COMPANIONS_ROOT` 覆盖（蜂窝/舵机等内嵌运行时用它把形象目录固定到各自数据目录）。当前仓库不内置任何形象包，列表为空是正常的。

## 精灵表规格

- 帧尺寸固定 **192 × 208**，同一动画状态的帧在一行内横向排列，**一个状态占一行**。
- 行号即状态（行 0–8）：

| 行 | 状态 | 行 | 状态 |
| --- | --- | --- | --- |
| 0 | idle | 5 | failed |
| 1 | running-right | 6 | waiting |
| 2 | running-left | 7 | running |
| 3 | waving | 8 | review |
| 4 | jumping | | |

- 会话运行状态映射到动画行：`running → running`、`done → jumping`、`pending → waiting`、`error → failed`，其余回落到 `idle`。点击形象播放 `waving` 一次性动画。
- 实际包可以少声明帧数：越界的行/帧会被边界检查降级处理，不会崩。

## 形态差异

| 形态 | 呈现方式 |
| --- | --- |
| 蜂窝（Windows） | 原生**浮窗**：每只宠物一个 Win32 分层窗口，独立消息线程驱动帧时钟与拖拽，不占 UI 线程 |
| 蜂窝（Linux） | **没有等价浮窗**，桌面宠物仅 Windows 可用 |
| 蜂巢 | 页面内浮层（`CompanionFloatingLayer`），跟随聊天状态 |
| 舰桥 | 形象管理面板（全局形象的上传、编辑、删除、导出） |
| 舵机 | 无形象界面；仅把 `WUNDER_COMPANIONS_ROOT` 指到本地数据目录 |

## 缩放与位置

| 参数 | 范围 | 出处语义 |
| --- | --- | --- |
| 存储/菜单缩放 | 0.5–1.6（预设档位 0.5/0.8/1.0/1.2/1.4/1.6） | 菜单与写入前夹紧 |
| 渲染缩放 | 0.5–1.8 | 允许图标负载携带渲染范围内的任意值 |
| 位置 | 按屏幕工作区夹紧并保持边距 | 拖拽结果持久化 |

蜂窝把位置、每智能体 `show/scale` 覆盖和全局消息提示开关持久化到 `companion.overlay.json`；蜂巢对应状态存浏览器 `localStorage`（清浏览器数据会重置位置与偏好）。

## 接口与治理边界

- 用户面只有 4 个只读 GET：`/wunder/companions/global`（列表）、`/{id}`（详情）、`/{id}/spritesheet`、`/{id}/package`（导出）。
- 增删改全部在管理面：`/wunder/admin/companions[...]`（列表/导入/单只读改删/精灵表/导出包），导入请求体上限与 ZIP 上限一致（24 MB）。
- 蜂巢的「私有形象」只在该浏览器本地生效，不上传服务端；全局形象由管理员经舰桥上传后所有用户可见。

## 常见误解

- **「形象是另一种智能体」**——不是。形象只是某个智能体的视觉外壳，一个智能体可以绑定一只形象。
- **「用户可以在蜂巢导入形象到服务器」**——不可以。用户面接口只读，服务端形象一律由管理员经舰桥管理；私有导入只在本地浏览器。
- **「Linux 蜂窝也有桌面浮窗」**——没有。浮窗是 Win32 分层窗口实现，Linux 无等价物。
- **「形象动画影响任务执行」**——无关。运行状态驱动动画行，反向不成立。
- **「系统自带一批形象包」**——当前工作树没有内置形象目录，需管理员自行上传。

## 相关

- [目标态](/docs/zh-CN/concepts/goal-mode/) — 形象「运行中」动画对应的目标延续轮次
- [舰桥面板指南](/docs/zh-CN/reference/admin-panels/) — 形象管理面板操作
- [智能体循环](/docs/zh-CN/concepts/core-agent-loop/) — 运行状态的真正来源
