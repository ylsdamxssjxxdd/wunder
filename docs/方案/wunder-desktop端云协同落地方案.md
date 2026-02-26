# wunder-desktop端云协同落地方案（正式版：云端主权 + 本地异步镜像）

## 0. 决策冻结

### 0.1 运行模式

- Desktop 首次启动默认 `local`（纯本地）。
- Desktop 连接服务器后进入 `hybrid_collab`（端云协同）。
- 纯云端模式仅在浏览器提供（`browser_cloud`），Desktop 不提供该入口。

### 0.2 执行与数据主权

- 端云协同下，智能体执行环境固定在云端。
- 云端工作区是权威数据源（Source of Truth）。
- 用户指定的本地目录是镜像目录（Mirror），用于查看和交付。

### 0.3 同步原则

- 智能体执行与本地同步解耦：同步由异步线程处理，不阻塞智能体。
- 用户断线期间，云端任务继续执行。
- 用户重连后继续同步 checkpoint 之后的 revision，云端覆盖本地。

### 0.4 首次目录映射原则（新增）

- 用户在端云协同中选择本地目录后，需执行一次**本地 -> 云端初始化映射（Bootstrap Seed）**。
- 初始化过程必须有可视化进度条与阶段状态。
- 初始化完成后切换为常态：云端执行 + 云端 -> 本地异步回写。

---

## 1. 现状基线（代码事实）

- 当前 `remote_gateway` 主要完成远端 API/WS 切换：`wunder-desktop/runtime.rs:341`、`wunder-desktop/bridge.rs:179`、`frontend/src/config/runtime.ts:90`
- 本地设置接口独立鉴权：`frontend/src/api/desktop.ts:1`
- 当前“执行命令”默认本地执行：`src/services/tools.rs:4810`
- 已有 `node.invoke` 机制可复用同步通道：`src/services/tools.rs:1516`、`src/gateway/mod.rs:367`

结论：需新增“初始化映射 + 常态异步镜像 + 进度可观测”三件套。

---

## 2. 目标架构

```text
┌─────────────────────────────────────────────────────────────┐
│ Cloud Control Plane                                         │
│ user/org/model/agent/session/policy/audit/user-world       │
└───────────────────────────────┬─────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────┐
│ Cloud Execution Plane                                       │
│ orchestrator + tools runtime + deps env                    │
│ output: revision + changeset/artifact/manifest             │
└───────────────────────────────┬─────────────────────────────┘
                                │ gateway node.invoke / sync api
┌───────────────────────────────▼─────────────────────────────┐
│ Local Projection Plane (Desktop Node Agent)                │
│ seed worker + sync worker + lock retry + atomic write      │
│ target: container mapped local roots                        │
└──────────────────────────────────────────────────────────────┘
```

模式矩阵：
- `desktop-local`：本地执行 + 本地文件。
- `desktop-hybrid`：云端执行 + 本地异步镜像（默认协同模式）。
- `browser-cloud`：云端执行 + 云端文件（无本地映射）。

---

## 3. 配置与协议（正式字段）

## 3.1 Desktop 运行配置

```json
{
  "runtime_mode": "local | hybrid_collab",
  "collab": {
    "enabled": true,
    "cloud_base_url": "https://server.company.com",
    "node_id": "node-001",
    "node_token": "***",
    "heartbeat_s": 15,
    "seed_worker_concurrency": 4,
    "sync_worker_concurrency": 4,
    "sync_retry_backoff_s": [5, 15, 30, 60],
    "sync_checkpoint": "rev_123456"
  }
}
```

## 3.2 容器映射配置

```json
{
  "container_mounts": [
    {
      "container_id": 1,
      "cloud_workspace_id": "cw_abc",
      "local_root": "D:/team/project",
      "readonly": false,
      "seed_status": "idle | running | done | failed"
    }
  ]
}
```

约束：
- Desktop-hybrid 下 `local_root` 必须写权限预检（创建、写入、重命名、删除）。
- 浏览器纯云端提交 `local_root` 必须被服务端拒绝。

## 3.3 同步命令（网关）

- 文本：`workspace.apply_patch`
- 二进制：`workspace.put_artifact`
- 批量：`workspace.apply_manifest`

统一字段：`container_id`、`relative_path`、`revision`、`request_id`、`trace_id`。

---

## 4. 首次映射（本地 -> 云端）与进度条设计（重点）

## 4.1 初始化流程（Seed Job）

用户绑定目录后触发 `seed_job`：
1. `discovering`：扫描目录树（过滤 deny_globs、系统文件、超大文件策略）。
2. `indexing`：计算清单（path/size/mtime/hash 可延迟计算）。
3. `uploading`：分片上传（建议 8MB chunk，支持断点续传）。
4. `verifying`：云端校验 hash 与文件数。
5. `committing`：生成基线 revision，写入 `sync_checkpoint`。
6. `done`：容器转入常态同步。

## 4.2 进度模型（前端必须展示）

进度条采用“阶段 + 百分比 + 速率 + ETA”四维展示。

```json
{
  "job_id": "seed_001",
  "container_id": 1,
  "stage": "uploading",
  "progress": {
    "percent": 63.4,
    "processed_files": 1288,
    "total_files": 2031,
    "processed_bytes": 536870912,
    "total_bytes": 847249203,
    "speed_bps": 12582912,
    "eta_seconds": 25
  },
  "current_item": "src/assets/logo.png"
}
```

前端呈现要求：
- 容器卡片顶部显示总进度条。
- 详情抽屉显示阶段、当前文件、速率、剩余时间、错误数。
- 大于 3 秒的阶段必须有可见动画与文案，避免“假死感”。

## 4.3 进度事件通道

- 本地 bridge 提供 `seed_progress` 事件流（WS 优先，SSE 兜底）。
- 事件最小频率 500ms，最大频率 2s（防抖 + 节流）。
- 客户端可断线重连并按 `job_id` 恢复显示。

## 4.4 用户交互控制

- 支持 `pause`/`resume`/`cancel`。
- cancel 后保留已上传块索引，可选择“继续上次任务”。
- 同步失败时展示“可重试”与失败原因（权限、占用、网络、配额）。

---

## 5. 常态同步（云 -> 本地）

### 5.1 线程模型

- 云端工具返回后立即结束主链路，不等待本地写盘。
- 本地同步 worker 独立处理：按容器并发、按文件串行。
- 状态机：`queued -> applying -> applied | retrying | failed_locked | failed_perm`。

### 5.2 一致性规则

- 云端为准；本地冲突由云端覆盖。
- 按 revision 严格顺序应用，防止旧版本覆盖新版本。
- 应用采用“临时文件 + 原子 rename”，避免半文件。

### 5.3 可选保护

- 可配置本地短期 shadow 备份（默认关闭，企业可开）。
- 仅用于误操作恢复，不参与一致性决策。

---

## 6. 锁文件与权限异常处理

### 6.1 锁文件（Word 等）

- 检测到占用 -> `locked_pending`。
- 进入重试队列，按退避策略重试。
- 解锁后自动覆盖并回写审计。

### 6.2 权限不足

- 标记 `failed_perm`，不阻塞其他文件同步。
- UI 高亮“需人工处理”，并提供“定位目录”“复制诊断信息”。

### 6.3 审计字段

记录：`job_id`、`revision`、`retry_count`、`last_error`、`locked_at`、`applied_at`。

---

## 7. 性能与容量评估

### 7.1 目标指标

- 智能体执行链路 P95 不因同步增加超过 100ms。
- Seed 上传吞吐可观测（MB/s），中断后续传成功率 >= 99%。
- 常态同步成功率 >= 99.9%（7 天滚动）。

### 7.2 关键优化

- 小文件批量聚合提交，降低 RTT。
- 大文件分片并发上传/下载。
- Hash 分级策略：小文件全量 hash，大文件先 size+mtime 再抽样 hash。
- 目录扫描与上传流水线并行，避免“先扫完再传”的长等待。

---

## 8. 代码改造落点

- Desktop runtime/bridge：`wunder-desktop/runtime.rs`、`wunder-desktop/bridge.rs`
- Desktop settings + sync jobs API：`src/api/desktop.rs`
- Orchestrator 执行后变更产出：`src/orchestrator/request.rs`、`src/orchestrator/execute.rs`
- 工具结果结构化：`src/services/tools.rs`
- Gateway 同步命令与状态：`src/gateway/mod.rs`、`src/api/admin.rs`
- 前端设置与进度 UI：`frontend/src/views/DesktopSystemSettingsView.vue`、`frontend/src/config/desktop.ts`

---

## 9. 分阶段实施

| 里程碑 | 周期 | 目标 |
|---|---:|---|
| M0 | 0.5 周 | 冻结字段与状态机（seed/sync/progress） |
| M1 | 1 周 | 目录映射、权限预检、seed job 骨架 |
| M2 | 1.5 周 | 分片上传+断点续传+阶段进度事件 |
| M3 | 1 周 | 云 -> 本地常态同步、revision 顺序应用 |
| M4 | 1 周 | 前端进度条、详情抽屉、失败重试交互 |
| M5 | 1 周 | 审计与管理端可观测（队列、吞吐、失败率） |
| M6 | 2 周 | 企业灰度、容量压测、GA |

---

## 10. 验收标准

- 用户选择本地目录后可看到初始化进度条（阶段、百分比、速率、ETA）。
- 初始化完成后容器进入“已同步”状态并可持续增量同步。
- 云端生成图片（如爱心图）可异步落地到本地映射目录。
- 断线期间云端任务不中断，重连后继续按 revision 同步。
- 锁文件/权限异常不阻塞智能体执行，且可见、可重试、可审计。

---

## 11. 文档联动

实施后同步更新：
- `docs/API文档.md`
- `docs/设计方案.md`
- `docs/系统介绍.md`

---

## 12. 当前已落地（本次实现）

- 新增 Desktop 初始化映射 API：
  - `POST /wunder/desktop/sync/seed/start`
  - `GET /wunder/desktop/sync/seed/jobs`
  - `GET /wunder/desktop/sync/seed/jobs/{job_id}`
  - `POST /wunder/desktop/sync/seed/control`
- `desktop/settings` 扩展 `container_mounts` 与 `container_cloud_workspaces`，并返回每个容器 `seed_status`。
- 初始化映射任务支持阶段进度（discovering/indexing/uploading/verifying/committing/done）与 `pause/resume/cancel` 控制。
- Desktop 设置页已提供：
  - 容器级 `cloud_workspace_id` 配置
  - Seed 状态标签 + 进度条 + ETA/速率
  - 启动/暂停/继续/取消按钮
  - 轮询任务状态
- 端云协同时依旧允许浏览本地目录并绑定容器路径。

