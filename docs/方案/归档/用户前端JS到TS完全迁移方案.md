# 用户侧前端 JS 到 TS 完全迁移方案（frontend）

> 版本：v1.1  
> 日期：2026-02-09  
> 说明：修复上一版文档中文乱码，并保留完整迁移节点与验收标准。

---

## 1. 迁移目标与边界

### 1.1 目标
- 将 `frontend/src` 中所有业务 `.js` 文件迁移到 `.ts`/`<script setup lang="ts">`，最终达成 **0 JS business file**。
- 在迁移过程中对高复杂模块进行结构化重构（特别是 chat 链路）。
- 保持现有交互与后端接口语义不变：WS 优先，SSE fallback，对话可中断/恢复，工作区与用户工具不回退。
- 构建长期门禁：`vue-tsc --noEmit` + `vite build` must pass。

### 1.2 边界
- 仅覆盖用户侧前端：`frontend`。
- 不包含管理端 `web` 的 TS 迁移。
- 不改 Rust 后端业务逻辑，仅做类型对齐与前端结构重构。

---

## 2. 代码审查结论（现状基线）

- JS files: **38** (`frontend/src`), total about **8900 LOC**.
- Vue SFC: **49**, `<script setup>` total about **11573 LOC**.
- 高风险热点：
  - `frontend/src/stores/chat.js` (3442 lines)
  - `frontend/src/views/ChatView.vue` script block (~1489 lines)
  - `frontend/src/components/chat/WorkspacePanel.vue` script block (~1931 lines)
  - `frontend/src/components/user-tools/UserKnowledgePane.vue` script block (~1398 lines)
- 全局高耦合点：
  - `@/i18n` used widely (55 refs)
  - `@/utils/apiError` used widely (20 refs)
  - Chat payload normalization has many dynamic branches (`?.`, `JSON.parse`, fallback aliases)
- 当前缺少 TS 基础设施：no `tsconfig` / `vue-tsc` / `*.d.ts` baseline.

---

## 3. 总体迁移策略

- 策略 1：**progressive migration**，先搭骨架再收紧规则。
- 策略 2：**domain-first**，先 low-risk modules，后 chat core。
- 策略 3：**refactor before typing**，大文件先拆再转 TS。
- 策略 4：建立领域类型中心（api/chat/workspace/user-tools/i18n），禁止敃点 any。

推荐目录：
```text
frontend/src/
  types/
    api.ts
    auth.ts
    agent.ts
    chat.ts
    workspace.ts
    user-tools.ts
    i18n.ts
  stores/chat/
    index.ts
    state.ts
    persist.ts
    stream.ts
    workflow.ts
    snapshot.ts
  composables/
    chat/
    workspace/
```

---

## 4. 分阶段节点计划（N0 ~ N8）

### N0 基线冻结（0.5 天）
**目标**：建立迁移前可对照基线。

**任务**
- Run and record: `npm run build`.
- 导出迁移资产清单（JS files / large SFC / critical flows）。
- 准备回归检查清单（chat/workspace/user-tools）。

**通过标准**
- Build baseline available.
- Regression checklist executable.

### N1 TS 基础设施（1 天）
**目标**：让 typecheck 可运行，但不阻断现有 JS。

**任务**
- Add dev deps: `typescript`, `vue-tsc`, `@types/node`.
- Add `frontend/tsconfig.json`, `frontend/tsconfig.node.json`.
- Add `frontend/src/env.d.ts`, `frontend/src/shims-vue.d.ts`.
- Convert `vite.config.js` -> `vite.config.ts`.
- Add scripts: `typecheck`, `build:check`.

**首期配置建议**
- `allowJs: true`
- `checkJs: false`
- `strict: true`
- `noEmit: true`
- `isolatedModules: true`

**通过标准**
- `npm run typecheck` runs and returns deterministic diagnostics.
- `npm run build` still passes.

### N2 领域类型骨架（1.5 天）
**目标**：先统一数据模型，降低后续返工。

**任务**
- `types/api.ts`: `ApiEnvelope<T>`, `PagedResult<T>`, error payload.
- `types/chat.ts`: `ChatSession`, `ChatMessage`, `MessageStats`, `ChatStreamEvent`.
- `types/workspace.ts`: entry/file/tree models.
- `types/user-tools.ts`: MCP/skill/knowledge models.
- `types/i18n.ts`: language code + key type.

**通过标准**
- API layer and store layer share the same domain types.
- New API usage without explicit type is not allowed.

### N3 低风险模块迁移（1.5 天）
**范围**
- `src/config/*.js`
- `src/utils/clipboard.js`
- `src/utils/maintenance.js`
- `src/utils/workspaceEvents.js`
- `src/utils/workspaceResources.js`
- `src/utils/toolSelection.js`
- `src/utils/toolSummary.js`
- `src/main.js`, `src/router/index.js`

**要点**
- Rename to `.ts` and add explicit in/out types.
- 补全 `CustomEvent` / route meta typing.

**通过标准**
- Zero type errors in migrated modules.
- Routing/login guard behavior unchanged.

### N4 API 层 + 基础 Store 迁移（2 天）
**范围**
- APIs except chat: `auth/admin/agents/channels/cron/swarm/userTools/workspace`.
- Stores: `auth/agents/admin/workspace/theme/performance`.
- `utils/apiError.js` to TS.

**要点**
- Type axios instance/interceptors.
- Type multipart upload signatures.
- Eliminate implicit any in state/getter/action.

**通过标准**
- Login/profile/workspace/user-tools/theme flows pass manual regression.

### N5 常规 SFC 迁移（2.5 天）
**目标**：非核心大文件先转 `lang="ts"`。

**要点**
- `defineProps<Props>()` / typed `defineEmits`.
- `ref<T>` and `computed<T>` nullability cleanup.
- Keep light/dark theme compatibility.

**通过标准**
- Migrated components build and typecheck pass.

### N6 聊天核心重构 + TS 迁移（4 天，最高优先级）
**范围**
- `src/stores/chat.js`
- `src/api/chat.js`
- `src/utils/ws.js`
- `src/utils/sse.js`
- `src/views/ChatView.vue`
- `src/components/chat/WorkspacePanel.vue`

**必须执行的拆分要求**
1. Split chat store into `state/persist/stream/workflow/snapshot`.
2. Extract `ChatView` composables: session lifecycle, stream lifecycle, ability panel, history virtualization.
3. Extract `WorkspacePanel` composables: tree, upload/download, DnD, icon map, preview.
4. Unify WS/SSE event model under typed discriminated unions.

**保底约束**
- Keep WS-first, SSE-fallback behavior.
- Keep cancel/resume behavior.
- Keep token/context counting semantics unchanged (记录上下文占用量，非总消耗量).

**通过标准**
- Chat main flow regression passes (send, tool call, cancel, resume, draft).
- No `@ts-ignore` in chat domain.

### N7 i18n 类型收口（1 天）
**任务**
- `i18n/messages/*.js` -> `*.ts` with `as const`.
- Derive `I18nKey` from `zh-CN`, verify `en-US` key completeness.
- Type `t(key)` strictly.
- Switch `allowJs` to `false`.

**通过标准**
- `frontend/src` has no business `.js`.
- i18n key consistency check passes.

### N8 最终验收与治理固化（1 天）
**任务**
- Run: `npm run typecheck`, `npm run build`.
- Add CI gate (typecheck + build).
- Output migration report and post-migration debt list.

**通过标准**
- Type errors: 0
- Build: pass
- Critical manual regression: pass

---

## 5. 优先级队列

### P0（先拆后转）
- `frontend/src/stores/chat.js`
- `frontend/src/views/ChatView.vue`
- `frontend/src/components/chat/WorkspacePanel.vue`
- `frontend/src/utils/ws.js`
- `frontend/src/api/chat.js`

### P1（高频业务）
- `frontend/src/components/user-tools/UserKnowledgePane.vue`
- `frontend/src/components/user-tools/UserMcpPane.vue`
- `frontend/src/components/user-tools/UserSkillPane.vue`
- `frontend/src/views/PortalView.vue`
- `frontend/src/views/ProfileView.vue`
- `frontend/src/utils/markdown.js`

### P2（基础设施）
- `frontend/src/config/*.js`
- `frontend/src/main.js`
- `frontend/src/router/index.js`
- `frontend/src/stores/{auth,admin,agents,workspace,theme,performance}.js`

---

## 6. 量化验收标准

- 代码指标
  - Business `.js` in `frontend/src`: **0**
  - `vue-tsc --noEmit`: **0 error**
  - `vite build`: **pass**
- 功能指标
  - Chat: create session, send message, ws/sse switch, cancel, resume
  - Workspace: upload/download/search/preview/move
  - User tools: MCP/skill/knowledge CRUD
- 规约指标
  - No new `@ts-ignore`
  - No performance-heavy UI regression (e.g. `backdrop-filter`)

---

## 7. 风险与对策

- 风险 1：chat 域一次性迁移面积过大。
  - 对策：强制执行 N6 拆分路线，每个子模块独立 typecheck + regression。
- 风险 2：i18n key 漂移。
  - 对策：加 key consistency script + CI gate。
- 风险 3：API 响应字段异构。
  - 对策：统一 `ApiEnvelope<T>` + adapter normalization。
- 风险 4：迁移后继续新增 JS。
  - 对策：`allowJs: false` + lint/CI gate。

---

## 8. 排期预估（单人）

- N0-N2: 3 天
- N3-N5: 6 天
- N6: 4 天
- N7-N8: 2 天

**总计：约 15 人天（含回归与修复缓冲）**

---

## 9. 执行建议

- 不建议 big-bang 一次性全改，按节点推进。
- 每个节点结束后都要能单独 build + regression。
- N6 前建议先补一批聊天链路回归脚本（至少覆盖 send/cancel/resume）。
