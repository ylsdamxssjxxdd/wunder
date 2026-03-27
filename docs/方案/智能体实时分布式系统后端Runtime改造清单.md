# 智能体实时分布式系统后端 Runtime 改造清单

## 1. 当前代码判断

基于代码现状，后端目前有四个核心事实：

- [runtime.rs](C:/Users/sjxx/Desktop/wunder/src/services/runtime/thread/runtime.rs) 已经承担了 thread 提交、main session 解析、lease、队列唤醒等多种职责。
- [runtime.rs](C:/Users/sjxx/Desktop/wunder/src/services/runtime/mission/runtime.rs) 仍是全局扫描型 MissionRuntime，不是 mission owner 模式。
- [x] [state.rs](C:/Users/sjxx/Desktop/wunder/src/core/state.rs) 已收敛为 `kernel / projection / control` 三层注入，旧的平铺 runtime/projection/control 字段已移除。
- [x] `presence` 已迁移到 [src/services/presence/](C:/Users/sjxx/Desktop/wunder/src/services/presence)；控制平面不再使用单体 `user_presence.rs`。

## 2. 后端改造目标

- 把 thread 执行变成 owner-based runtime。
- 把 mission 执行变成实例化 runtime。
- 把 projection/presence/directory 从执行主链路中拆开。
- 让 `AppState` 从“服务平铺”演进成“kernel/projection/control”分层注入。

## 3. Thread Runtime 清单

### 3.1 从现有 AgentRuntime 拆出的职责

- [ ] `submit_user_request` 迁入 thread runtime manager。
- [ ] `resolve_main_session_id` / `resolve_or_create_main_session_id` 迁入 thread registry。
- [ ] `set_main_session` 迁入 thread binding service。
- [ ] `pending_sessions` 与 `running_threads` 收敛成 thread owner state。
- [ ] `stream_events` 的 thread 公共事件改由 `thread/public_events.rs` 统一发射。

### 3.2 Thread Runtime 一期收口内容

- [ ] 旧入口直接改接 thread runtime，不再保留并行实现。
- [ ] 保留 wake/queue 接口，但语义统一归入 thread runtime 主路径。
- [ ] 删除旧 `AgentRuntime` 名称与文件残留，避免双主语并存。

### 3.3 需要新增的后端文件

- [ ] `src/services/runtime/thread/runtime.rs`
- [ ] `src/services/runtime/thread/state.rs`
- [ ] `src/services/runtime/thread/submit.rs`
- [ ] `src/services/runtime/thread/public_events.rs`
- [ ] `src/services/runtime/thread/checkpoint.rs`

### 3.4 需要修改的文件

- [ ] [runtime.rs](C:/Users/sjxx/Desktop/wunder/src/services/runtime/thread/runtime.rs)
- [ ] [chat_ws.rs](C:/Users/sjxx/Desktop/wunder/src/api/chat_ws.rs)
- [ ] [chat.rs](C:/Users/sjxx/Desktop/wunder/src/api/chat.rs)
- [ ] [state.rs](C:/Users/sjxx/Desktop/wunder/src/core/state.rs)

### 3.5 Thread Runtime 验收

- [ ] 任一 thread 都能查到 owner 与 epoch。
- [ ] 同一 thread 不会同时存在两个执行器。
- [ ] 取消、重试、恢复都只经由 thread owner 生效。

## 4. Mission Runtime 清单

### 4.1 从当前 TeamRunRunner 拆出的职责

- [ ] `queued -> running -> merging -> terminal` 状态机独立成 mission runtime。
- [ ] task assign / task result / merge / finalize 从全局 runner 拆出。
- [ ] `active_runs` 与 `sessions` 状态从 `runner.rs` 迁入 mission runtime state。
- [ ] 任务裁决与对外投影分离。

### 4.2 一期切换方案

- [ ] [runner.rs](C:/Users/sjxx/Desktop/wunder/src/services/swarm/runner.rs) 直接切为 mission runtime 拆分入口。
- [ ] 移除 `legacy | hybrid | mission_runtime` 三档切换思路，统一只保留 mission runtime 主路径。
- [ ] `SwarmService` 与 `team_runs` API 直接改调新 runtime。

### 4.3 需要新增的文件

- [ ] `src/services/runtime/mission/runtime.rs`
- [ ] `src/services/runtime/mission/assignment.rs`
- [ ] `src/services/runtime/mission/scheduler.rs`
- [ ] `src/services/runtime/mission/merge.rs`
- [ ] `src/services/runtime/mission/public_summary.rs`

### 4.4 需要修改的文件

- [x] [runtime.rs](C:/Users/sjxx/Desktop/wunder/src/services/runtime/mission/runtime.rs)
- [ ] [team_runs.rs](C:/Users/sjxx/Desktop/wunder/src/api/team_runs.rs)
- [ ] [beeroom.rs](C:/Users/sjxx/Desktop/wunder/src/api/beeroom.rs)
- [x] [beeroom.rs](C:/Users/sjxx/Desktop/wunder/src/services/projection/beeroom.rs)

### 4.5 Mission Runtime 验收

- [ ] mission 热点不会拖垮普通 chat thread。
- [ ] task claim 和 merge 顺序稳定。
- [ ] mission 结束后可快速得到 public summary 和 replay checkpoint。

## 5. Projection / Publisher 清单

### 5.1 目标

- 让 beeroom/user_world/session 都变成 runtime 的公开投影。

### 5.2 需要新增的文件

- [ ] `src/services/projection/beeroom_room.rs`
- [ ] `src/services/projection/user_world_room.rs`
- [ ] `src/services/projection/session_room.rs`
- [ ] `src/services/projection/publisher.rs`
- [ ] `src/services/projection/snapshot.rs`

### 5.3 需要修改的现有文件

- [x] [beeroom.rs](C:/Users/sjxx/Desktop/wunder/src/services/projection/beeroom.rs)
- [ ] [user_world.rs](C:/Users/sjxx/Desktop/wunder/src/services/user_world.rs)
- [ ] [beeroom_ws.rs](C:/Users/sjxx/Desktop/wunder/src/api/beeroom_ws.rs)
- [ ] [user_world_ws.rs](C:/Users/sjxx/Desktop/wunder/src/api/user_world_ws.rs)

### 5.4 验收

- [ ] beeroom 和 user_world 的投影可以独立恢复。
- [ ] projection 延迟上升时不影响 thread/mission 主执行。

## 6. Directory / Presence 清单

### 6.1 Directory

- [ ] 新增 `thread_routes`、`mission_routes`、`projection_routes` 路由 lease 逻辑。
- [ ] 新增 `AgentDirectoryService`。
- [ ] 所有执行命令先找 owner，再执行业务。

### 6.2 Presence

- [x] 将 `presence` 拆成：
  - connection presence
  - watch presence
- [x] 网关/WS 连接与 watch 订阅分开记录。
- [x] presence 不再只服务联系人在线提示，还服务投影路由和降级。

### 6.3 验收

- [ ] 多实例下在线态不分裂。
- [ ] watch 数量、订阅目标、隐藏页订阅降级都可观测。

## 7. AppState 与路由收口清单

### 7.1 AppState

- [x] 调整 [state.rs](C:/Users/sjxx/Desktop/wunder/src/core/state.rs) 的注入顺序：
  - directory
  - thread runtime
  - mission runtime
  - projection publisher
  - presence
- [x] 避免继续平铺添加“越来越多的服务单例”。

### 7.2 API 路由

- [ ] 保留 [mod.rs](C:/Users/sjxx/Desktop/wunder/src/api/mod.rs) 当前入口不变。
- [ ] 新增 `realtime_world.rs`，统一 target-aware 协议。
- [ ] 旧 `chat_ws` / `beeroom_ws` / `user_world_ws` 先转发到新 runtime 层。

### 7.3 验收

- [ ] 旧客户端可用。
- [ ] 新 runtime 协议可逐步接管。

## 8. 后端测试清单

- [ ] Thread owner 单写者测试
- [ ] Mission assign/finalize 顺序测试
- [ ] Projection snapshot/replay 测试
- [ ] Route lease fencing 测试
- [ ] Presence connect/watch 生命周期测试
- [ ] 慢客户端 backpressure 降级测试
