# 每线程模型切换：去掉输入区切模型的配置写盘

## Context

输入区切换模型当前被实现为「修改智能体配置模型」：每轮执行时模型只从 agent 记录与全局默认解析（`resolve_chat_model_name`），线程没有自己的模型字段，因此临时换模型必须写 agent 记录（蜂巢写库+全量刷新，蜂窝写 config.yaml）。语义错位：临时切换被放大成改默认配置，还波及该智能体的其他线程。

方案：照搬 reasoning_effort 的现成模式——**线程级模型覆盖存 workspace meta**（`session_reasoning_effort:{user}:{session}` 同款机制，零 schema 迁移），解析链改为 `session 覆盖 → agent.model_name → config.llm.default`。切换只写一条 meta，天然秒切；乐观 UI 已在上一轮就位，本轮把持久化目标从 agent 改为 session 覆盖，并删除 agent 补丁链。

## 后端 wunder-runtime

1. **meta 存取**（`src/services/workspace.rs`，仿 L1993-2022 的 reasoning_effort 函数）
   - `load_session_model_override(user_id, session_id) -> Option<String>`
   - `save_session_model_override(user_id, session_id, model: Option<&str>)`（None/空串=清除）
   - key：`session_model:{user}:{session}`；删除会话时随 reasoning_effort 一起清理（L775-794 附近）。

2. **解析函数**（`src/services/agent_execution.rs`）
   - 新增 `resolve_thread_model_name(config, agent_record, session_override: Option<&str>)`：override 非空且在 `config.llm.models` 中且 `is_llm_model` 才生效，否则逐级回退；`resolve_chat_model_name` 保留为委托（= override 为 None）。

3. **执行链接入**（唯一模型解析点，HTTP/WS/native 共用）
   - `src/api/chat.rs:542`（`build_chat_request`）：`let selected_model_name = resolve_thread_model_name(&config, agent_record.as_ref(), state.workspace.load_session_model_override(&user.user_id, &session_id).as_deref());`（reasoning_effort 已在此处同样读 meta，模式照抄 L529-536）
   - `src/api/chat.rs:641`（`native_chat_context_capacity`）：同样传入 override（不同模型 max_context 不同）。
   - `src/api/chat/prompt.rs:315/391` 是系统提示词**预览**路径，不入执行链，保持不动。
   - `src/services/cron.rs:2002` 定时任务不属于线程，不动。

4. **会话 API**（`src/api/chat/sessions.rs`，仿 reasoning-effort 端点 L546-580）
   - 新端点 `POST /wunder/chat/sessions/{session_id}/model`，body `{ "model_name": String }`（空=清除回退）；路由注册在 L78-80 旁；保存时校验模型存在于 config 且是 LLM（错误返回与 reasoning-effort 端点同风格）；返回 `{data:{id, model_name}}`。
   - 会话详情 payload（L521-529）与列表扩展（L328-336）追加 `session_model_name`（仅覆盖存在时携带）；`resolve_session_model_runtime`（L1744-1764）改为经 `resolve_thread_model_name` 解析，使详情里的 `model_name`/`model_key` 反映线程覆盖。

## 蜂窝（wunder-desktop + frontend-slint）

5. **native facade**（`crates/wunder-desktop/native.rs`，仿 `save_session_reasoning_effort` L510-528）
   - `set_session_model(session_id, model_key: &str)`：校验线程存在后写 meta；空串清除。
   - `NativeSession` 增加 `model_name`（有效模型，仿 L452-456 从 workspace 读 override 并解析），会话载荷带给 UI。
   - 仿 `tests/preferences_facade.rs:129-148` 加持久化测试。

6. **UI 接线**（frontend-slint）
   - `ui/main.slint:1018` composer 的 `select-model` 改绑新回调 `root.select-thread-model(key)`（新增 callback），不再走 `set-default-model`；设置页的「设为默认」仍用 `set-default_model`（上一轮的乐观改动保留）。
   - native_pages.rs（或 native_chat.rs）绑定：乐观投影标签（复用 `select_model_key`）→ 后台 `api.set_session_model(active_session_id, key)` → 失败回滚旧 key 并报错。
   - 线程切换/加载时：session 带覆盖则 `select_model_key` 投影标签，否则回落 agent 模型（现有 `selected-agent-model` 链）。已知耦合：标签投影同时改设置页编辑器选中项，属现有行为，可接受。

## 蜂巢（frontend）

7. **api**（`src/api/chat.ts`，仿 L163 reasoning-effort）
   - 新增 `updateSessionModel(id, model_name)` → `api.post(/chat/sessions/${id}/model, ...)`。

8. **controller**（`src/views/messenger/controller/messengerControllerInstall.ts`）
   - `applyComposerModelSelection` 重写：乐观补丁目标从 agent 档案改为**本地 session 记录**（chatStore，patch `model_name`/`session_model_name`），调用新端点，失败回滚并报错；删除上一轮的 agent 补丁 + 写入链（`composerModelWriteChain`/`composerModelPatchEpoch` 随之移除）。
   - `agentHeaderModelDisplayName`（L2944）优先级插入线程覆盖：session 覆盖 → agent 配置 → session 运行时 → agent 运行时。

## 验证

- `cargo check -p wunder-runtime -p wunder-desktop -p wunder-frontend-slint` + `wunder-desktop` preferences facade 测试。
- 蜂巢 `npx vue-tsc --noEmit`。
- 手工链路：输入区切模型 → 菜单即关、标签同帧变化；发一条消息确认实际请求用了新模型（tool_logs/线程轨迹模型名）；换回后回退 agent 配置；模型被删除后旧覆盖自动回落不报错；其他线程不受影响；定时任务仍用 agent/全局模型。
- 完成后写入 docs/功能迭代.md（性能+变更），并把「每线程模型覆盖」约定补进项目记忆。
