# MessengerView 拆分方案

> 背景：`frontend/src/views/MessengerView.vue` 当前 8391 行（模板约 1-1746 行，脚本约 1747-8890 行，watch/生命周期集中在 6500+ 行）。
> 目标：单文件不超过 2000 行；`MessengerView.vue` 仅保留组合与装配职责，功能与交互保持一致。

## 现状梳理（按模块）
- 模板：左侧导航栏 + 中栏列表 + 主聊天区（设置/消息/应用广场）+ 右侧 Dock + 多个 Dialog/Popover。
- 脚本：状态量、computed、watch/生命周期 + 语音录制/播放 + markdown/资源加载 + 消息虚拟滚动 + 工具/文件/联系人/群组/智能体逻辑。
- 已有拆分基础：`views/messenger/` 下已有 `model.ts` / `orgUnits.ts` / `worldHistory.ts` / `worldVoice.ts`。

## 拆分原则
1. 先模板、后逻辑；每一步都可编译通过。
2. 不改 UI/行为，仅做结构调整。
3. 领域收敛：每个 composable 负责一组 state + methods + watchers。
4. 新文件命名与目录保持清晰可检索。

## 目标目录结构（建议）
```
frontend/src/views/messenger/
  MessengerView.vue                // 组合壳，< 1200 行
  sections/
    MessengerLeftRail.vue
    MessengerMiddlePane.vue
    MessengerChatPanel.vue
    MessengerRightDockHost.vue
    MessengerDialogsHost.vue
  chat/
    MessengerChatHeader.vue
    MessengerChatSettings.vue
    MessengerAgentMessageList.vue
    MessengerWorldMessageList.vue
    MessengerHelperWorkspace.vue
  composables/
    useMessengerLayout.ts
    useMessengerNavigation.ts
    useMessengerProfileAvatar.ts
    useMessengerAgents.ts
    useMessengerAgentRuntime.ts
    useMessengerWorld.ts
    useMessengerWorldVoice.ts
    useMessengerTools.ts
    useMessengerFiles.ts
    useMessengerMessages.ts
    useMessengerMarkdown.ts
    useMessengerWorkspaceResources.ts
    useMessengerTimeline.ts
    useMessengerDesktop.ts
    useMessengerBootstrap.ts
```
（可根据实际合并/裁剪，原则是单文件 < 2000 行）

## 模板拆分映射
- `MessengerLeftRail.vue`
  - 负责：头像、左侧导航、辅助应用入口、设置入口。
  - 依赖：`currentUserAvatar*`、`leftRail*Options`、`isLeftNavSectionActive`。
  - 事件：`openProfile/openSettings/openHelperApps/switchSection`。
- `MessengerMiddlePane.vue`
  - 负责：中栏标题/搜索/列表区域。
  - 进一步拆分（子组件）：
    - `MessagesList`（对话列表）
    - `UsersList`（组织树 + 联系人虚拟列表）
    - `GroupsList`
    - `AgentsList`（列表/网格）
    - `ToolsList`
    - `FilesList`
    - `MoreList`
- `MessengerChatPanel.vue`
  - 负责：聊天主区域容器 + header + body + footer。
  - 子组件：
    - `MessengerChatHeader.vue`（标题/模型信息/头部按钮）
    - `MessengerChatSettings.vue`（设置视图：agents/users/groups/tools/files/more）
    - `MessengerAgentMessageList.vue` / `MessengerWorldMessageList.vue`（消息渲染与虚拟滚动）
    - `MessengerHelperWorkspace.vue`（应用广场）
- `MessengerRightDockHost.vue`
  - 负责：右侧 Dock 的两种形态（Agent/Group），统一折叠逻辑。
- `MessengerDialogsHost.vue`
  - 负责：各类弹窗/面板（Timeline/History/Prompt/Image/GroupCreate/ContainerPicker/FileMenu…）。

## 逻辑拆分（composables 建议职责）
- `useMessengerLayout.ts`
  - `viewportWidth` / overlay / middlePane / rightDockCollapsed。
  - pointer/resize 事件 + overlay hide 逻辑。
- `useMessengerNavigation.ts`
  - `sectionOptions` / `switchSection` / route 同步 / `activeSectionTitle`。
- `useMessengerProfileAvatar.ts`
  - 头像选项与本地持久化、当前用户头像样式。
- `useMessengerAgents.ts`
  - `agentMap` / `selectedAgent` / 会话切换 / 能力概要 / prompt 预览。
- `useMessengerAgentRuntime.ts`
  - runtimeState / cron/channel 标记 / 运行记录入口。
- `useMessengerWorld.ts`
  - contacts/groups / worldDraft / history / emoji / container picker。
- `useMessengerWorldVoice.ts`
  - world 语音录制/播放/时长等。
- `useMessengerTools.ts`
  - tools catalog / category / admin tool 详情。
- `useMessengerFiles.ts`
  - 文件容器列表 / WorkspacePanel / 容器菜单 / desktop 容器管理。
- `useMessengerMessages.ts`
  - message list / virtual scroll / scroll to bottom / pending center。
- `useMessengerMarkdown.ts`
  - markdown cache / renderAgentMarkdown / renderWorldMarkdown。
- `useMessengerWorkspaceResources.ts`
  - workspace 资源解析/加载/缓存/预览下载。
- `useMessengerTimeline.ts`
  - timeline 预览/详情弹窗/右侧 dock session history。
- `useMessengerDesktop.ts`
  - desktop 模式能力、更新检查、截图能力。
- `useMessengerBootstrap.ts`
  - bootstrap/定时器/状态刷新，统一 `onMounted/onBeforeUnmount`。

## 拆分步骤与节点（建议节奏）
节点 1：模板拆分完成（不动逻辑）
- 产出：`sections/` 与 `chat/` 子组件落地，`MessengerView.vue` 仅保留 props/emit 连接。
- 验收：页面功能一致；`MessengerView.vue` 行数降至 ~3000。

节点 2：布局与导航逻辑抽离
- 产出：`useMessengerLayout` / `useMessengerNavigation` / `useMessengerProfileAvatar`。
- 验收：overlay/route 同步行为一致；无新增 watch 警告。

节点 3：消息渲染与虚拟滚动抽离
- 产出：`useMessengerMessages` + Agent/World MessageList 子组件。
- 验收：滚动定位、底部吸附、虚拟高度一致。

节点 4：世界会话域拆分
- 产出：`useMessengerWorld` / `useMessengerWorldVoice` + 相关 dialog/quick panel。
- 验收：语音录制/播放、emoji、附件选择一致。

节点 5：智能体域拆分
- 产出：`useMessengerAgents` / `useMessengerAgentRuntime`。
- 验收：会话切换、模型显示、能力预览一致。

节点 6：工具/文件/桌面能力拆分
- 产出：`useMessengerTools` / `useMessengerFiles` / `useMessengerDesktop` / `useMessengerTimeline`。
- 验收：工具页、文件容器、desktop 更新/截图功能一致。

节点 7：bootstrap & 清理收尾
- 产出：`useMessengerBootstrap`，集中 timers/watchers；`MessengerView.vue` < 1200 行。
- 验收：`onMounted/onBeforeUnmount` 无遗漏；所有新文件 < 2000 行。

## 风险点与注意事项
- watch 顺序与依赖：拆分时确保同一领域 watcher 与 state 同文件，避免循环触发。
- 虚拟滚动缓存：`messageVirtualHeightCache` 的生命周期必须与消息切换一致。
- 语音播放/录制：切换会话时的清理顺序需保持一致。
- Workspace 资源缓存：切换会话/文件容器时必须清理 cache。
- Dialog/Popover：保持 `teleport` 与 ref 关系，避免定位与关闭事件失效。
