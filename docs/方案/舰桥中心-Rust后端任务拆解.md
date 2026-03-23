# 舰桥中心 Rust 后端任务拆解（已落地版）

## 1. 当前结论

舰桥中心后端主链路已经落地，不再处于“从零规划”阶段。

当前真实实现位置：

- 管理 API：`src/api/admin_bridge.rs`
- 桥接服务：`src/services/bridge/service.rs`
- 渠道接线：`src/channels/service.rs`
- runtime 注入：`src/core/state.rs`
- 存储抽象：`src/storage/bridge.rs`
- 存储实现：`src/storage/sqlite.rs`、`src/storage/postgres.rs`

说明：

- 旧草稿中出现的 `/wunder/admin/bridge_centers/*` 路径已废弃
- 当前真实接口统一收口为 `/wunder/admin/bridge/*`
- 产品命名已经统一为：`舰桥中心 -> 舰桥节点`

---

## 2. 已完成模块

### 2.1 存储层

已完成：

- `bridge_centers`
- `bridge_center_accounts`
- `bridge_user_routes`
- `bridge_delivery_logs`
- `bridge_route_audit_logs`

已覆盖：

- SQLite 建表与 CRUD
- Postgres 建表与 CRUD
- `channel/account_id` 唯一约束
- `center_account_id + external_identity_key` 唯一路由约束

### 2.2 服务层

已完成：

- 外部身份提取
- 自动开户
- 自动确保默认预设智能体
- bridge route 创建/复用
- 入站/出站投递日志
- route 活跃时间更新

核心函数：

- `resolve_inbound_bridge_route(...)`
- `auto_provision_route(...)`
- `touch_bridge_route_after_outbound(...)`
- `log_bridge_delivery(...)`

### 2.3 渠道主链路接线

已完成：

- 手工绑定/用户绑定未命中时进入 bridge 解析
- bridge 命中后把 `route.agent_id` 注入正常会话解析链路
- 会话 metadata 挂载 `bridge_center_id / bridge_center_account_id / bridge_route_id`
- 出站时从 outbox/session metadata 回捞 bridge route 并记录 delivery log

路由优先级：

1. `channel_bindings / channel_user_bindings`
2. `bridge route`
3. `owner fallback`

### 2.4 管理 API

已完成：

- 元数据接口
- 支持渠道目录接口
- 节点列表/详情/保存/删除
- 接入渠道列表/单条增删改
- 路由列表/详情/状态切换
- 投递日志查询

额外完成：

- 节点保存支持一次性提交 `shared_channels[]`
- 同步时优先按 `(channel, account_id)` 复用已有接入渠道记录，避免因漏传 `center_account_id` 误删历史路由和日志

---

## 3. 管理端页面落地情况

已完成：

- 页面总称：`舰桥中心`
- 单个实体：`舰桥节点`
- 单模型页面：`节点配置 + 接入渠道 + 自动分配路由 + 投递日志`
- 接入渠道由节点统一保存，不再暴露“共享账号绑定”独立步骤

页面文件：

- `web/index.html`
- `web/modules/bridge-center.js`
- `web/modules/elements.js`
- `web/app.js`

---

## 4. 当前剩余工作

### 4.1 必做

- 增加完整集成测试，覆盖：
  - 共享渠道首包入站
  - 自动开户
  - 自动确保预设智能体
  - route 建立
  - 出站 delivery log

### 4.2 建议做

- 管理端增加更明确的错误提示和表单校验
- 增加 route detail 的更多审计信息展开
- 增加不同 provider 的联调样例

---

## 5. 验收标准

满足以下条件即可认为舰桥中心后端闭环可用：

1. 管理员能在舰桥中心中新建舰桥节点并保存接入渠道
2. 外部用户首次从共享渠道发消息时自动开户
3. 系统自动为该用户确保默认预设智能体
4. 后续消息稳定命中同一 bridge route
5. 回复沿原共享渠道账号回发成功
6. 管理端可看到路由与投递日志

---

## 6. 当前校验记录

已通过：

- `cargo check --release`
- `node --check web/modules/bridge-center.js`
- `node --check web/app.js`

已补的轻量测试：

- `BridgeCenterUpsertPayload` 支持 `shared_channels[]`
- 接入渠道记录复用/冲突解析 helper 测试
