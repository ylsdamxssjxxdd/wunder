# 舰桥中心 Rust 后端任务拆解

## 1. 目标

这份文档回答一件事：

- 如果现在开始按现有方案落地 `舰桥中心`，Rust 后端应该先改什么、后改什么、具体落到哪些文件、迁移顺序怎么排、每一步怎么验收。

这里的范围只覆盖后端：

- 存储模型
- 服务层
- ChannelHub 接入
- 管理端 API
- 测试与发布顺序

不覆盖：

- 管理端前端实现细节
- 用户侧页面改造
- 最终视觉设计

---

## 2. 落地原则

落地时必须守住下面四条：

1. 外部桥接用户最终落到真实 `user_accounts`，不新造 bridge runtime 用户。
2. 共享渠道账号凭证继续只保存在 `channel_accounts`，不复制到每个用户。
3. 桥接只新增“外部身份 -> wunder 用户 -> agent”逻辑路由层。
4. 现有手工绑定优先级保持最高：`manual binding > bridge_center > owner fallback`。

---

## 3. 代码落点总览

## 3.1 新增文件

建议新增这些文件：

- `src/storage/bridge.rs`
  - 存放 bridge 相关 record、query、enum、DTO
- `src/services/external/mod.rs`
  - 外链共享服务模块入口
- `src/services/external/provision.rs`
  - 从 `auth.rs` 提取 `provision_external_launch_session(...)`
  - 从 `auth.rs` 提取 `resolve_or_create_external_embed_agent(...)`
- `src/services/bridge/mod.rs`
- `src/services/bridge/identity.rs`
- `src/services/bridge/provision.rs`
- `src/services/bridge/router.rs`
- `src/services/bridge/read_model.rs`
- `src/services/bridge/logs.rs`
- `src/channels/bridge_router.rs`
- `src/api/admin_bridge.rs`

建议新增测试文件：

- `tests/bridge_storage_smoke.rs`
- `tests/bridge_center_admin_api.rs`
- `tests/bridge_center_inbound_provision.rs`
- `tests/bridge_center_routing_priority.rs`

## 3.2 必须最小接线的现有文件

这些文件需要改，但应保持“最小接线”，不要把大量新逻辑继续堆进去：

- `src/storage/mod.rs`
  - 挂接 `mod bridge;`
  - 暴露 bridge record/query
  - 为 `StorageBackend` 增加 bridge trait 方法
- `src/storage/sqlite.rs`
  - 新表 DDL
  - SQLite bridge CRUD 实现
- `src/storage/postgres.rs`
  - 新表 DDL
  - Postgres bridge CRUD 实现
- `src/services/mod.rs`
  - 暴露 `external`、`bridge`
- `src/channels/mod.rs`
  - 暴露 `bridge_router`
- `src/channels/service.rs`
  - 在现有主链路中插入 bridge 路由解析调用
- `src/api/mod.rs`
  - 注册 `admin_bridge::router()`
- `src/api/auth.rs`
  - 改为复用 `services/external/provision.rs`

## 3.3 大文件约束下的取舍

仓库规则要求不要继续向超大文件堆功能。

但这里有两个现实限制：

- `src/storage/sqlite.rs`
- `src/storage/postgres.rs`

当前 schema 与持久化实现就集中在这里，所以 bridge 表的 DDL 与 trait 实现不可避免要改这两个文件。

正确策略不是回避，而是：

- 只把“必要的 schema/CRUD 接线”放进去；
- 把 bridge 的 record/query/业务逻辑尽量外移到新文件；
- 后续若存储层持续扩展，再考虑独立拆分 `sqlite_bridge.rs / postgres_bridge.rs`。

---

## 4. 分阶段任务拆解

## 阶段 A：提取共享外链能力

目标：

- 先把 `auth.rs` 里已经可复用的“自动开户”和“自动确保默认预设智能体”下沉到服务层；
- 避免舰桥中心直接依赖 API 私有函数。

### 任务 A1

新增：

- `src/services/external/mod.rs`
- `src/services/external/provision.rs`

迁移内容：

- `provision_external_user(...)`
- `provision_external_launch_session(...)`
- `resolve_or_create_external_embed_agent(...)`
- `resolve_external_embed_target_agent_name(...)`

保留在 `auth.rs` 的内容：

- HTTP request/response struct
- external auth key 校验
- launch/token exchange API 编排

### 任务 A2

改动：

- `src/services/mod.rs`
- `src/api/auth.rs`

结果要求：

- `auth.rs` 不再持有这些核心实现；
- 舰桥中心后续可直接调用 service。

### 阶段 A 验收

- `external/launch`
- `external/token_launch`
- `external/login`

行为不变，现有测试不回归。

---

## 阶段 B：存储模型落地

目标：

- 先把 bridge 数据表和 CRUD 能力补齐；
- 此时先不上线主路由切换，只把存储层打通。

### 任务 B1：新增 bridge records / queries

新增：

- `src/storage/bridge.rs`

建议定义：

- `BridgeCenterRecord`
- `BridgeCenterAccountRecord`
- `BridgeUserRouteRecord`
- `BridgeDeliveryLogRecord`
- `BridgeRouteAuditLogRecord`

建议定义查询对象：

- `ListBridgeCentersQuery`
- `ListBridgeCenterAccountsQuery`
- `ListBridgeUserRoutesQuery`
- `ListBridgeDeliveryLogsQuery`
- `ListBridgeAuditLogsQuery`

建议定义轻量统计对象：

- `BridgeCenterStatsRecord`
- `BridgeRouteResolveResult`

### 任务 B2：扩展 `StorageBackend`

改动：

- `src/storage/mod.rs`

新增 trait 方法建议：

- `upsert_bridge_center(...)`
- `get_bridge_center(...)`
- `list_bridge_centers(...)`
- `delete_bridge_center(...)`
- `upsert_bridge_center_account(...)`
- `list_bridge_center_accounts(...)`
- `get_bridge_center_account(...)`
- `delete_bridge_center_account(...)`
- `upsert_bridge_user_route(...)`
- `get_bridge_user_route_by_identity(...)`
- `get_bridge_user_route(...)`
- `list_bridge_user_routes(...)`
- `update_bridge_user_route_status(...)`
- `insert_bridge_delivery_log(...)`
- `list_bridge_delivery_logs(...)`
- `insert_bridge_route_audit_log(...)`
- `list_bridge_route_audit_logs(...)`

### 任务 B3：SQLite DDL 与实现

改动：

- `src/storage/sqlite.rs`

新增表：

1. `bridge_centers`
2. `bridge_center_accounts`
3. `bridge_user_routes`
4. `bridge_delivery_logs`
5. `bridge_route_audit_logs`

SQLite 必备索引：

- `idx_bridge_center_accounts_center`
- `idx_bridge_center_accounts_channel_account`
- `idx_bridge_routes_center_status_last_seen`
- `idx_bridge_routes_center_identity`
- `idx_bridge_routes_user`
- `idx_bridge_delivery_center_created`
- `idx_bridge_delivery_route_created`

### 任务 B4：Postgres DDL 与实现

改动：

- `src/storage/postgres.rs`

保持与 SQLite 语义一致：

- 同样的表名
- 同样的唯一键
- 同样的状态枚举约束
- 同样的查询能力

### 阶段 B 验收

需要至少补这些测试：

- SQLite 下 bridge center CRUD
- SQLite 下 route 并发唯一键
- Postgres 下 bridge center CRUD
- Postgres 下 route 查询排序/过滤

此阶段完成后，即使 ChannelHub 还没接入，也应该已经能在存储层完整读写 bridge 数据。

---

## 阶段 C：Bridge Service 业务层

目标：

- 在服务层完成“身份提取 -> 自动开户 -> 自动确保 agent -> 路由写入 -> 日志写入”。

### 任务 C1：身份提取模块

新增：

- `src/services/bridge/identity.rs`

职责：

- 根据 provider 与共享账号策略生成 `external_identity_key`
- 产出统一结构：
  - `external_identity_key`
  - `external_user_key`
  - `external_display_name`
  - `external_peer_id`
  - `external_sender_id`
  - `external_thread_id`

注意：

- 这个模块必须是 provider-agnostic；
- provider 特性通过策略参数传入，不能在这里写满 `if channel == xmpp` 的分支泥团。

### 任务 C2：自动开户与 route ensure

新增：

- `src/services/bridge/provision.rs`

职责：

- 根据中心配置与共享账号配置生成候选 wunder 用户名
- 调用 `services/external/provision.rs`
  - 自动创建/复用用户
  - 自动确保默认预设智能体
- 原子写入 `bridge_user_routes`
- 记录 audit log

这里要明确：

- 共享渠道账号挂接后，bridge 路由创建失败不能 silent fallback；
- route ensure 必须幂等；
- 同一 `center_account_id + external_identity_key` 只能成功创建一条 route。

### 任务 C3：路由读写与查询封装

新增：

- `src/services/bridge/router.rs`
- `src/services/bridge/read_model.rs`

职责划分：

- `router.rs`
  - 运行态 resolve
  - route create / status update
  - 路由优先级控制
- `read_model.rs`
  - 给管理端列表、详情、统计使用

### 任务 C4：日志服务

新增：

- `src/services/bridge/logs.rs`

职责：

- 写入 inbound/outbound delivery log
- 记录 route failure / provider failure / provision failure
- 给管理端投递日志页提供聚合查询

### 阶段 C 验收

至少补这些单元/服务测试：

- 合法 provider 身份提取
- 群聊场景 `sender_in_peer` 身份稳定性
- 非法用户名时自动回退生成 bridge 用户名
- 用户已存在时复用，不重复开户
- agent 已存在时复用，不重复创建
- route 并发 ensure 不产生重复记录

---

## 阶段 D：ChannelHub 主链路接入

目标：

- 让 bridge 真正参与渠道消息路由；
- 但只通过最小 hook 接入 `ChannelHub`。

### 任务 D1：新增 bridge_router

新增：

- `src/channels/bridge_router.rs`

职责：

- 在渠道主链路里封装 bridge 路由解析，不把细节堆进 `service.rs`

建议接口：

```rust
async fn resolve_bridge_route(...) -> Result<Option<ResolvedBridgeRoute>>
```

返回内容建议：

- `route_source`
- `route_id`
- `wunder_user_id`
- `agent_id`
- `created_user`
- `created_agent`
- `created_route`

### 任务 D2：在 `ChannelHub` 中插入路由优先级

改动：

- `src/channels/service.rs`

接入顺序必须是：

1. 手工 `channel_user_bindings / channel_bindings`
2. `bridge_router`
3. owner fallback

注意：

- 对已挂桥接中心的共享账号，如果 bridge resolve 失败，应立即终止，不再走 owner fallback；
- 这条判断必须非常明确，不能隐式穿透。

### 任务 D3：投递日志接线

当消息：

- 进入 bridge resolve
- 命中 route
- 自动开户成功/失败
- 出站成功/失败

都应写 `bridge_delivery_logs`。

### 阶段 D 验收

需要至少补这些集成测试：

- `manual binding > bridge > owner fallback` 优先级正确
- 首包自动开户成功
- 二次消息复用同一个 route
- 桥接失败时不会回退 owner
- provider 不支持主动出站时返回明确错误

---

## 阶段 E：管理端 API

目标：

- 先做只读治理与轻量控制；
- 不做用户智能体设置类接口。

### 任务 E1：新增 `admin_bridge.rs`

新增：

- `src/api/admin_bridge.rs`

建议先实现：

- `GET /wunder/admin/bridge_centers/supported_channels`
- `GET /wunder/admin/bridge_centers`
- `POST /wunder/admin/bridge_centers`
- `GET /wunder/admin/bridge_centers/{center_id}`
- `PUT /wunder/admin/bridge_centers/{center_id}`
- `POST /wunder/admin/bridge_centers/{center_id}/status`
- `GET /wunder/admin/bridge_centers/{center_id}/accounts`
- `POST /wunder/admin/bridge_centers/{center_id}/accounts`
- `PUT /wunder/admin/bridge_centers/{center_id}/accounts/{center_account_id}`
- `DELETE /wunder/admin/bridge_centers/{center_id}/accounts/{center_account_id}`
- `GET /wunder/admin/bridge_centers/{center_id}/routes`
- `GET /wunder/admin/bridge_centers/{center_id}/routes/{route_id}`
- `POST /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/status`
- `GET /wunder/admin/bridge_centers/{center_id}/delivery_logs`
- `GET /wunder/admin/bridge_centers/{center_id}/audit_logs`

### 任务 E2：接入主 router

改动：

- `src/api/mod.rs`

新增：

- `pub mod admin_bridge;`
- `build_router(...).merge(admin_bridge::router())`

说明：

- 这是 admin 路由，不需要挂入 desktop reduced router。

### 阶段 E 验收

需要补这些 API 测试：

- 创建中心
- 绑定共享账号
- 已被占用账号返回冲突
- 查询 route 列表
- 路由暂停/恢复/封禁
- 查询投递日志

---

## 阶段 F：发布前测试与验收

目标：

- 在真正切入生产主链路前，保证 bridge 不破坏原渠道行为。

### 必做测试矩阵

1. 存储层
   - SQLite CRUD / 索引 / 唯一键
   - Postgres CRUD / 过滤 / 排序
2. 服务层
   - 身份提取
   - 自动开户
   - agent ensure
   - route 幂等
3. ChannelHub 集成
   - 手工绑定优先
   - bridge 命中
   - owner fallback
   - bridge fail no fallback
4. API 层
   - 管理端中心 CRUD
   - 共享账号挂接
   - route 状态控制
   - 日志查询

### 推荐验收场景

- `xmpp` 首包桥接
- `weixin` context_token 回复链路
- `feishu` 群聊发送者隔离
- `qqbot` 用户桥接
- `wechat / wechat_mp` 单用户会话桥接

这里不是说一期必须把每个 provider 都做单独特化逻辑，而是至少要验证：

- 同一桥接主模型能在多个 provider 上跑通。

---

## 5. migration 顺序

这里的重点不是“写哪张表”，而是“上线顺序不能把渠道主链路打断”。

推荐顺序如下。

## migration 1：只上表，不接主路由

改动：

- `src/storage/mod.rs`
- `src/storage/bridge.rs`
- `src/storage/sqlite.rs`
- `src/storage/postgres.rs`

内容：

- 新增 bridge 五张表
- 新增索引
- 新增 trait CRUD

此时：

- 生产运行行为不变
- 仅 schema 扩展

## migration 2：抽离 external service

改动：

- `src/services/external/*`
- `src/api/auth.rs`

此时：

- 外链行为不变
- bridge 可开始复用 shared service

## migration 3：上 bridge service + admin API，但不挂 ChannelHub

改动：

- `src/services/bridge/*`
- `src/api/admin_bridge.rs`
- `src/api/mod.rs`

此时：

- 管理端可创建中心、挂共享账号、查空路由
- 但入站消息还不会自动桥接

## migration 4：接入 ChannelHub 主链路

改动：

- `src/channels/bridge_router.rs`
- `src/channels/mod.rs`
- `src/channels/service.rs`

此时：

- bridge 真正生效
- 需要与现有渠道回归测试一起上线

## migration 5：补日志、审计、统计与只读联调

改动：

- `src/services/bridge/logs.rs`
- `src/services/bridge/read_model.rs`
- `admin_bridge` 查询接口补全

此时：

- 页面可完整观测
- 便于运营与排障

---

## 6. 推荐开发顺序

如果是单人连续开发，建议按下面顺序推进：

1. `services/external` 提取
2. `storage/bridge.rs + StorageBackend trait`
3. `sqlite/postgres` bridge 表与 CRUD
4. `services/bridge/identity.rs`
5. `services/bridge/provision.rs`
6. `services/bridge/router.rs`
7. `api/admin_bridge.rs`
8. `channels/bridge_router.rs`
9. `channels/service.rs` 最小接线
10. bridge 集成测试

这个顺序的好处是：

- 每一步都能单独验证；
- 不会一上来就碰 `ChannelHub` 主链路；
- 出问题时更容易定位在 storage / service / api / routing 哪一层。

---

## 7. 不建议的落地方式

不要这样做：

1. 直接在 `src/api/admin.rs` 里追加几千行 bridge 管理逻辑
2. 直接在 `src/channels/service.rs` 里写完整 bridge 生命周期
3. 直接在 `src/api/auth.rs` 里继续复制 external launch 逻辑
4. 为了省事，把共享账号复制成每个用户一条 `channel_account`
5. 先做前端页面，再倒逼后端补接口

这些路径都会让后续维护成本迅速失控。

---

## 8. 交付节点建议

## 节点 1：存储与 admin 空壳

完成标准：

- 表已建
- CRUD 已通
- 管理端能创建中心和绑定共享账号

## 节点 2：bridge 自动开户闭环

完成标准：

- 指定 provider 首包可自动创建真实用户
- 默认密码 `123456`
- 自动确保默认预设智能体
- 写入稳定 route

## 节点 3：多 provider 回归

完成标准：

- 至少 3 个 provider 验证 bridge 主模型可复用
- 手工绑定优先级正确
- silent fallback 已禁止

## 节点 4：日志与运维观测

完成标准：

- delivery logs 可查
- audit logs 可查
- route 状态可控制

---

## 9. 最终建议

真正关键的不是“先把多少接口写出来”，而是把实现顺序控稳：

- 先抽外链共享能力
- 再补表
- 再做 bridge service
- 再挂主链路
- 最后补管理查询与观测

这样做，`舰桥中心` 的后端落地才不会把当前渠道系统打乱，也不会把大文件继续堆到不可维护。
