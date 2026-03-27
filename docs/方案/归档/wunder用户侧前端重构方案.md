# wunder 用户侧前端重构方案（聊天软件形态）

## 1. 目标与边界

### 1.1 重构目标
- 用户侧前端从“多页面门户”重做为“IM 聊天软件主界面”，以聊天为第一入口。
- 视觉与交互完全借鉴 `C:\Users\32138\Desktop\参考项目\HuLa-v3.0.9`，并以你提供的截图作为最终界面验收基线。
- 功能逻辑复用 `frontend-old`（智能体、会话、用户世界、工具链、工作区、WS/SSE 流式能力）。
- 智能体被视为“可聊天对象”（等价独立用户），统一进入会话体系。
- i18n 只保留中文/英文（`zh-CN`、`en-US`）。

### 1.2 约束
- `frontend-old` 已弃用，仅作功能对照，不再增量修补。
- 前端禁用一切 backdrop 相关样式（`backdrop-filter`、`-webkit-backdrop-filter` 及其变体）。
- 不保留 light/dark 两套主题变量，改为单一 HuLa 风格主题（固定样式体系）。
- 通讯策略保持 WebSocket 优先、SSE 兜底。
- 兼容 web 与 desktop（desktop runtime 注入与远程模式保留）。

---

## 2. 代码现状分析（已完成）

### 2.1 wunder 现状（`frontend-old`）

1) 业务能力完整但入口割裂  
- 智能体入口在 `PortalView`，聊天在 `ChatView`，用户/群聊在 `UserWorldView`，文件与工具又分散在其它页面。  
- 对应文件：  
  - `frontend-old/src/views/PortalView.vue`（智能体卡片、创建/编辑/删除）  
  - `frontend-old/src/views/ChatView.vue`（智能体聊天）  
  - `frontend-old/src/views/UserWorldView.vue`（用户/群聊聊天）  

2) 核心数据层可直接复用  
- `frontend-old/src/stores/chat.ts`：会话管理、消息流、WS/SSE 自动切换、续传、计划/问询面板等能力成熟。  
- `frontend-old/src/stores/userWorld.ts`：用户世界会话、群聊、消息推送、WS/SSE 监听完整。  
- `frontend-old/src/components/chat/WorkspacePanel.vue`：工作区文件树、上传下载、编辑、批处理完整。  

3) 智能体创建操作可直接迁移  
- `PortalView` 中“新建智能体”具备完整字段与行为：`copy_from_agent_id`、`tool_names`、`sandbox_container_id`、`is_shared`。  
- 该能力应迁移到新界面搜索框右侧“+”菜单。

4) 主题与国际化改造点  
- i18n 可沿用中英能力；主题体系不沿用旧的浅/深双主题，改为单主题重建。  

### 2.2 参考项目（HuLa）可复用点

1) 主体布局范式成熟  
- `src/layout/index.vue`：Left + Center + Right 三栏模型，支持异步加载与窗口收缩。  
- `src/layout/left/index.vue`：头像 + 图标导航栏交互。  
- `src/layout/center/index.vue`：搜索框 + “+”按钮 + 列表容器。  
- `src/layout/right/index.vue`：聊天主区域承载。

2) 会话列表与聊天壳交互成熟  
- `src/views/homeWindow/message/index.vue`：会话列表视觉与状态表达（未读、置顶、状态图标）。  
- `src/components/rightBox/chatBox/index.vue`：头部、消息区、输入区、右侧信息栏组合方式。  
- `src/components/rightBox/chatBox/ChatSidebar.vue`：右侧扩展信息区（可承接 wunder 的沙盒/时间线/设置）。

3) 风险点（需规避）
- 参考项目大量使用 `backdrop-filter` 与较重视觉效果，本次必须全部移除并替换为纯色/阴影方案。  
- 参考项目桌面端（Tauri）特定能力较多，不可整包搬运。

---

## 3. 重构总策略

采用“**界面壳重建 + 业务内核复用 + 统一会话抽象**”：

1) 重建聊天软件式 UI 壳（按 HuLa + 截图风格）  
- 在不引入 HuLa 业务耦合的前提下，视觉结构、控件密度、留白与动效节奏尽量同构。

2) 复用 wunder 业务核心  
- 优先复用 `api/*`、`stores/chat.ts`、`stores/userWorld.ts`、`WorkspacePanel`、智能体创建/编辑逻辑。

3) 新增统一会话层（关键）  
- 把“智能体会话”和“用户世界会话”聚合成统一列表模型，在 UI 中一体呈现。  
- 聊天窗口按会话类型路由到对应 store，避免重写全部消息协议。

---

## 4. 目标信息架构（IA）

## 4.1 三栏布局
- **左栏（窄栏）**：头像 + 一级导航图标（固定 64px 左右）。  
- **中栏（列表栏）**：搜索、加号、动态列表（消息/用户/群/智能体/工具/文件/更多）。  
- **右栏（主工作区）**：聊天头部 + 消息流 + 输入区 + 右侧扩展区（沙盒/时间线/设置）。

### 4.2 左栏菜单定义
1. 头像（个人资料入口）  
2. 消息列表  
3. 用户列表  
4. 群聊列表  
5. 智能体列表  
6. 工具设置  
7. 文件管理器  
8. 更多设置（语言、主题、账号、关于、退出）

### 4.3 中栏顶部（搜索 + 加号）
- 搜索：按当前一级菜单搜索对应资源。  
- “+”按钮：主动作改为“新建智能体”（与旧 Portal 行为一致）。  
- 若扩展二级动作，统一走弹出菜单，但“新建智能体”固定置顶。

### 4.4 右栏扩展区（重点迁移）
将旧智能体聊天页里的能力统一迁移到右侧区域：
- 沙盒容器：复用 `WorkspacePanel`，根据当前 agent/container 上下文切换。  
- 时间线：复用 `MessageWorkflow` 数据源，做独立时间线视图。  
- 设置选项：迁移 `FeatureAgentSettingsDialog` / `FeatureCronDialog` / `FeatureChannelDialog` 的入口与结构。

### 4.5 截图对齐视觉基线（新增）
- 左侧为 HuLa 风格竖向导航条（品牌区 + 头像 + 图标组），中栏为会话列表，右侧为聊天主区 + 成员侧栏。
- 会话列表采用“搜索框 + 加号 + 卡片列表”结构；当前会话卡片为高亮绿底、圆角较大、信息密度与截图一致。
- 聊天主区包含：顶部频道标题区、公告横条、消息气泡区、底部工具栏/输入区、发送按钮右下角对齐。
- 右侧成员区固定显示公告摘要与在线成员列表（头像 + 名称 + 状态），视觉与截图一致。
- 动效只保留轻量 hover/active/展开动画，不使用毛玻璃相关效果。

---

## 5. 关键功能映射（旧 -> 新）

1) 智能体应用卡片迁移  
- 来源：`PortalView` 智能体卡片区。  
- 目标：中栏“智能体列表”页签。  
- 保留：默认智能体卡 + Owned/Shared 分组 + 运行状态 + 渠道/定时任务标识 + sandbox id。

2) 新建智能体行为保持不变  
- 来源：`PortalView.openCreateDialog/saveAgent`。  
- 目标：中栏搜索框右侧“+”触发。  
- 保留字段：`name`、`description`、`system_prompt`、`tool_names`、`copy_from_agent_id`、`is_shared`、`sandbox_container_id`。

3) 聊天能力整合  
- 智能体聊天：沿用 `chat store + /chat/sessions/* + /chat/ws`。  
- 用户/群聊：沿用 `userWorld store + /user_world/* + /user_world/ws`。  
- UI 统一呈现，后端协议按类型分流。

4) 工具设置与文件管理
- 工具设置：整合原 user-tools、channels、cron 入口。  
- 文件管理：复用 `WorkspacePanel` 作为独立列表页与右侧扩展页双入口。

---

## 6. 技术方案细节

### 6.1 新 frontend 目录结构（建议）

```text
frontend/
  src/
    app/
      AppShell.vue
      layout/
        LeftRail.vue
        MiddlePane.vue
        RightWorkspace.vue
        RightDock.vue
    modules/
      conversations/
      agents/
      contacts/
      groups/
      tools/
      files/
      settings/
    stores/
      sessionHub.ts        # 新增：统一会话聚合层
      chat.ts              # 迁移并裁剪自 frontend-old
      userWorld.ts         # 迁移并裁剪自 frontend-old
      agents.ts
      auth.ts
      ui.ts
    api/                   # 迁移 frontend-old/api
    i18n/
      index.ts
      messages/zh-CN.ts
      messages/en-US.ts
    styles/
      tokens.css
      hula-like.css
      layout.scss
```

### 6.2 统一会话模型（新增）

```ts
type ConversationKind = 'agent' | 'direct' | 'group';

type ConversationListItem = {
  id: string;             // kind + raw id
  kind: ConversationKind;
  title: string;
  avatar?: string;
  unread: number;
  lastMessage: string;
  lastAt: number;
  pinned?: boolean;
  muted?: boolean;
  sourceId: string;       // chat.session_id 或 user_world.conversation_id
  agentId?: string;       // kind=agent 时存在
};
```

- `sessionHub` 只做聚合与排序，不替代底层 chat/userWorld store。  
- 点击会话后按 `kind` 分派到对应 store 加载消息。  

### 6.3 主题与样式
- 不再维护 light/dark 双主题，采用单一 HuLa 风格主题（固定色板与控件样式）。  
- 样式实现以“截图一致性”优先：颜色、圆角、边框、阴影、间距直接对齐参考项目与截图。  
- 禁止所有 backdrop 样式，统一改用：  
  - 半透明纯色背景（rgba）  
  - 低成本 box-shadow  
  - 短时长 transform/opacity 过渡动画

### 6.4 国际化
- 仅 `zh-CN`、`en-US`，移除多语言动态扩展。  
- 保留 `x-wunder-language` 请求头。  
- 新 key 按模块拆分：`nav.*`、`chat.*`、`agent.*`、`user.*`、`group.*`、`workspace.*`、`settings.*`。

### 6.5 通讯与容灾
- 继续沿用：WS 优先，SSE fallback。  
- 会话流式状态、恢复、停止逻辑沿用旧 `chat.ts`。  
- 用户世界实时事件沿用旧 `userWorld.ts` 的 watch 机制。

---

## 7. 分阶段实施节点（里程碑）

## 节点 A：工程骨架与依赖
- 新建 `frontend`，完成 Vite + Vue3 + TS + Pinia + Router + Naive UI（或兼容方案）初始化。  
- 接入 runtime config（`/config.json`、desktop bootstrap）。  
- 验收：登录页可跑通，语言切换可用，单主题样式骨架生效。

## 节点 B：三栏壳与导航
- 完成 Left/Middle/Right 三栏布局与收缩逻辑。  
- 左栏八个一级菜单可切换；中栏容器可按菜单切片渲染。  
- 验收：布局、动画、响应式行为接近参考项目。

## 节点 C：会话聚合层
- 落地 `sessionHub`，聚合 agent/direct/group 三类会话。  
- 完成消息列表页签（未读、排序、置顶、搜索）。  
- 验收：列表数据正确，点击可进入对应会话。

## 节点 D：智能体列表与新建入口
- 将旧 Portal 智能体卡迁入“智能体列表”。  
- 搜索框右侧“+”接入“新建智能体”（行为与旧版一致）。  
- 验收：创建后即时出现在列表，可一键进入聊天。

## 节点 E：聊天主区与输入流
- 接入聊天头部/消息流/输入区，复用 `chat.ts` 与 `userWorld.ts`。  
- 保留附件、命令输入、流式中断/续传能力。  
- 验收：agent 与用户/群会话都可稳定收发消息。

## 节点 F：右侧扩展区迁移
- 右侧区域提供 Tab：`沙盒`、`时间线`、`设置`。  
- 接入 `WorkspacePanel`、workflow timeline、agent/channel/cron 设置入口。  
- 验收：原 ChatView 对应功能可在右侧完成操作。

## 节点 G：工具设置/文件管理/更多设置
- 完成工具设置页、文件管理页、更多设置页（语言、账号、关于、退出）。  
- 验收：左栏所有菜单可用，核心路径完整闭环。

## 节点 H：联调与发布
- 全链路联调（server + desktop + web）。  
- 性能优化（虚拟列表、懒加载、缓存策略）与回归测试。  
- 验收：满足替换 `frontend-old` 的发布标准。

---

## 8. 验收标准（DoD）

1) 结构与视觉  
- 三栏架构稳定；布局、交互、动画风格与参考项目一致性高。  
- 无 `backdrop-filter`。

2) 核心能力  
- 智能体聊天、用户聊天、群聊、智能体创建、沙盒文件操作、时间线与设置全部可用。  
- WS 优先与 SSE 兜底可验证。

3) 数据一致性  
- 会话未读、置顶、最后消息、时间排序正确。  
- 智能体视作聊天对象时，不破坏原有 session/user_world 语义。

4) 兼容性  
- Web 与 Desktop 均可启动运行。  
- i18n 仅中英，单主题在不同窗口尺寸下视觉一致。

---

## 9. 风险与应对

1) 双会话体系融合复杂  
- 应对：只新增聚合层，不改底层协议；分类型路由到原 store。

2) 参考项目依赖过重  
- 应对：只复用布局与可直接落地组件，不整包迁移生态。

3) 性能与长稳风险  
- 应对：列表虚拟化、按需加载、缓存 TTL、无高代价视觉特效。

4) 迁移期间可用性  
- 应对：`frontend-old` 保留对照，分节点替换，最后一次性切流。

---

## 10. 本方案落地顺序建议

建议按 A→H 顺序推进，每个节点独立验收并记录截图/接口清单；优先先打通“智能体会话 + 用户会话 + 右侧扩展区”三条主链，再补工具与更多设置。
