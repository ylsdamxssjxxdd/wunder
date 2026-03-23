# 舰桥中心 API 清单

## 1. 资源命名

- 页面总称：`舰桥中心`
- 单个实体：`舰桥节点`
- 节点接入渠道：`bridge_center_account`
- 自动分配路由：`bridge_route`

当前真实接口以 `src/api/admin_bridge.rs` 为准。

---

## 2. 元数据接口

### `GET /wunder/admin/bridge/metadata`

用途：初始化舰桥中心页面。

返回重点：

- `data.default_password`：自动开户默认密码，当前固定 `123456`
- `data.supported_channels[]`：支持渠道目录，含 `channel / display_name / description / docs_hint / adapter_registered / provider_caps`
- `data.preset_agents[]`：当前可选默认预设智能体
- `data.org_units[]`：可选目标单位
- `data.channel_accounts[]`：当前系统已激活渠道账号概览

### `GET /wunder/admin/bridge/supported_channels`

用途：单独获取渠道目录与能力快照。

---

## 3. 舰桥节点接口

### `GET /wunder/admin/bridge/centers`

用途：获取舰桥节点列表。

查询参数：

- `status`
- `keyword`
- `offset`
- `limit`

返回重点：

- `center_id`
- `name`
- `status`
- `default_preset_agent_name`
- `target_unit_id`
- `owner_user_id`
- `owner_username`
- `account_count`
- `shared_channel_count`
- `route_count`
- `active_route_count`
- `created_at`
- `updated_at`

### `POST /wunder/admin/bridge/centers`

用途：创建或更新舰桥节点。

当前管理端主表单字段：

- `center_id`：更新时传入
- `name`
- `code`
- `status`
- `default_preset_agent_name`
- `target_unit_id`
- `description`
- `default_identity_strategy`
- `username_policy`
- `settings`

说明：

- 页面已经把 `code / default_identity_strategy / username_policy` 隐藏，前端会按规则自动生成或写固定值。
- 后端仍接受 `shared_channels[]` 一次性提交，但当前舰桥中心页面不再走这条交互，而是由“接入渠道”弹窗逐条保存。

### `GET /wunder/admin/bridge/centers/{center_id}`

用途：查看单个舰桥节点详情。

返回重点：

- `data.center`
- `data.shared_channels[]`
- `data.accounts[]`

### `DELETE /wunder/admin/bridge/centers/{center_id}`

用途：删除舰桥节点，并级联清理节点下路由和日志。

---

## 4. 节点接入渠道接口

### `GET /wunder/admin/bridge/centers/{center_id}/accounts`

用途：获取某个节点的接入渠道列表。

### `POST /wunder/admin/bridge/centers/{center_id}/accounts`

用途：为某个节点新增接入渠道。

字段：

- `center_account_id`
- `channel`
- `account_id`
- `enabled`
- `default_preset_agent_name_override`
- `identity_strategy`
- `thread_strategy`
- `reply_strategy`
- `fallback_policy`
- `status_reason`

说明：

- 管理端在调用这个接口前，会先通过 `/wunder/channels/accounts?user_id=bridge_center_owner__{center_id}` 创建或更新节点专属渠道账号。
- 桥接策略和物理账号配置是分两步写入的。

### `PATCH /wunder/admin/bridge/accounts/{center_account_id}`

用途：更新某个节点接入渠道的桥接策略。

### `DELETE /wunder/admin/bridge/accounts/{center_account_id}`

用途：删除某个节点接入渠道，并清理其名下 bridge route 和日志。

---

## 5. 路由与日志接口

### `GET /wunder/admin/bridge/routes`

用途：查看自动分配路由。

查询参数：

- `center_id`
- `center_account_id`
- `channel`
- `account_id`
- `status`
- `keyword`
- `wunder_user_id`
- `agent_id`
- `offset`
- `limit`

### `GET /wunder/admin/bridge/routes/{route_id}`

用途：查看单条路由详情、最近投递日志、最近治理审计。

### `PATCH /wunder/admin/bridge/routes/{route_id}`

用途：治理路由状态。

支持字段：

- `status`：`active / paused / blocked / error`
- `clear_last_error`

### `GET /wunder/admin/bridge/delivery_logs`

用途：查看最近投递日志。

查询参数：

- `center_id`
- `center_account_id`
- `route_id`
- `direction`
- `status`
- `limit`

---

## 6. 页面与接口的实际对应关系

舰桥中心管理端当前是“监控主页 + 2 个弹窗”：

1. 主页调用 `/wunder/admin/bridge/centers`、`/wunder/admin/bridge/routes`、`/wunder/admin/bridge/delivery_logs`
2. `中心配置` 弹窗调用 `/wunder/admin/bridge/centers`
3. `接入渠道` 弹窗先调用 `/wunder/channels/accounts`，再调用 `/wunder/admin/bridge/centers/{center_id}/accounts` 或 `/wunder/admin/bridge/accounts/{center_account_id}`

这就是当前真实 API 使用方式。
