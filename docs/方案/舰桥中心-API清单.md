# 舰桥中心 API 清单

## 1. API 设计原则

舰桥中心 API 只服务管理员侧桥接治理，不复制用户侧和智能体侧控制台。

原则：

- API 对所有渠道 provider 通用；
- API 以“中心 / 共享账号 / 路由 / 投递日志”为主资源；
- API 不提供桥接用户智能体提示词编辑、线程切换、定时任务管理、工作区编辑；
- 若需要深入操作，应跳转现有用户页、线程页、工作区页；
- 路由控制只允许桥接级动作，例如：暂停、恢复、封禁。

---

## 2. 资源模型

| 资源 | 说明 |
|---|---|
| `bridge_center` | 一个舰桥中心 |
| `bridge_center_account` | 一个挂到舰桥中心的共享渠道账号 |
| `bridge_route` | 一个外部身份到 wunder 用户/agent 的稳定映射 |
| `bridge_delivery_log` | 一条投递日志 |
| `bridge_audit_log` | 一条系统或管理员动作日志 |

---

## 3. 目录与能力探测 API

## 3.1 查询舰桥可用渠道目录

- `GET /wunder/admin/bridge_centers/supported_channels`

用途：

- 返回 `ChannelCatalog` 中所有可用于舰桥中心的 provider；
- 同时返回适配器是否已注册、当前是否可绑定、是否已有共享账号被占用。

响应示例：

```json
{
  "data": {
    "items": [
      {
        "channel": "xmpp",
        "display_name": "XMPP",
        "user_supported": true,
        "catalog_present": true,
        "adapter_registered": true,
        "runtime_mode": "runtime+generic",
        "bindable": true,
        "docs_hint": "/wunder/channel/xmpp/webhook"
      },
      {
        "channel": "slack",
        "display_name": "Slack",
        "user_supported": true,
        "catalog_present": true,
        "adapter_registered": false,
        "runtime_mode": "generic",
        "bindable": false,
        "docs_hint": "/wunder/channel/slack/webhook"
      }
    ]
  }
}
```

---

## 4. 中心管理 API

## 4.1 查询中心列表

- `GET /wunder/admin/bridge_centers`

查询参数建议：

- `status`
- `q`
- `offset`
- `limit`

返回字段建议：

- `center_id`
- `name`
- `code`
- `status`
- `default_preset_agent_name`
- `accounts_count`
- `routes_count`
- `active_routes_count`
- `last_activity_at`

## 4.2 创建中心

- `POST /wunder/admin/bridge_centers`

请求体示例：

```json
{
  "name": "客服总入口",
  "code": "service_hub",
  "description": "统一承接外部渠道用户",
  "default_preset_agent_name": "客服助手",
  "target_unit_id": "unit_support",
  "default_identity_strategy": "sender_in_peer",
  "username_policy": "namespaced_generated"
}
```

返回字段建议：

- `center_id`
- `created_at`
- `updated_at`

## 4.3 查询中心详情

- `GET /wunder/admin/bridge_centers/{center_id}`

返回字段建议：

- 中心主数据
- 汇总统计
- 最近错误摘要
- 默认预设信息

## 4.4 更新中心

- `PUT /wunder/admin/bridge_centers/{center_id}`

允许修改：

- `name`
- `description`
- `default_preset_agent_name`
- `target_unit_id`
- `default_identity_strategy`
- `username_policy`

不允许在这里修改：

- 具体某个 bridge 用户的 agent 配置
- 具体某个 bridge 用户的线程配置

## 4.5 中心启停

- `POST /wunder/admin/bridge_centers/{center_id}/status`

请求体示例：

```json
{
  "status": "paused"
}
```

支持：

- `active`
- `paused`
- `disabled`

---

## 5. 共享账号挂接 API

## 5.1 查询中心下的共享账号

- `GET /wunder/admin/bridge_centers/{center_id}/accounts`

返回字段建议：

- `center_account_id`
- `channel`
- `account_id`
- `enabled`
- `default_preset_agent_name_override`
- `identity_strategy`
- `thread_strategy`
- `reply_strategy`
- `status_reason`
- `runtime_snapshot`
- `last_activity_at`

## 5.2 绑定共享账号

- `POST /wunder/admin/bridge_centers/{center_id}/accounts`

请求体示例：

```json
{
  "channel": "weixin",
  "account_id": "shared_service_bot",
  "default_preset_agent_name_override": null,
  "identity_strategy": "platform_user",
  "thread_strategy": "main_thread",
  "reply_strategy": "provider_bound"
}
```

语义要求：

- 必须引用已存在的 `channel_accounts`；
- 若 `(channel, account_id)` 已被其他中心占用，则返回冲突；
- 不在这个接口里写入或修改渠道凭证。

## 5.3 更新共享账号挂接策略

- `PUT /wunder/admin/bridge_centers/{center_id}/accounts/{center_account_id}`

允许修改：

- `enabled`
- `default_preset_agent_name_override`
- `identity_strategy`
- `thread_strategy`
- `reply_strategy`

## 5.4 删除共享账号挂接

- `DELETE /wunder/admin/bridge_centers/{center_id}/accounts/{center_account_id}`

删除前建议返回影响预估：

- 当前路由数
- 最近 24h 活跃路由数
- 是否仍有未完成 outbox

可拆成：

- `GET /wunder/admin/bridge_centers/{center_id}/accounts/{center_account_id}/impact`
- `DELETE ...`

---

## 6. 路由查询与桥接控制 API

## 6.1 查询路由列表

- `GET /wunder/admin/bridge_centers/{center_id}/routes`

查询参数建议：

- `channel`
- `center_account_id`
- `status`
- `q`
- `wunder_user_id`
- `agent_id`
- `updated_from`
- `updated_to`
- `offset`
- `limit`

列表字段建议：

- `route_id`
- `channel`
- `account_id`
- `external_display_name`
- `external_identity_key`
- `wunder_user_id`
- `agent_id`
- `agent_name`
- `status`
- `first_seen_at`
- `last_seen_at`
- `last_error`

## 6.2 查询路由详情

- `GET /wunder/admin/bridge_centers/{center_id}/routes/{route_id}`

详情字段建议：

- 路由基础信息
- 外部身份快照
- wunder 用户与智能体信息
- 最近投递统计
- 最近 session/thread 链接
- 最近错误与最近日志摘要

## 6.3 路由状态控制

- `POST /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/status`

请求体示例：

```json
{
  "status": "blocked",
  "reason": "external spam"
}
```

允许动作：

- `active`
- `paused`
- `blocked`

说明：

- 这是 bridge 路由状态，不是 `user_agents.status`；
- 这里只做接入控制，不做 agent 设置。

## 6.4 查询路由关联投递日志

- `GET /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/deliveries`

查询参数建议：

- `direction`
- `status`
- `offset`
- `limit`

## 6.5 查询路由跳转链接

- `GET /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/links`

返回：

- 用户详情页链接
- 线程详情页链接
- 渠道账号详情页链接

这个接口的目的是把“深入操作”明确导流到现有页面，而不是在舰桥中心复制一套编辑逻辑。

---

## 7. 日志与统计 API

## 7.1 中心统计

- `GET /wunder/admin/bridge_centers/{center_id}/stats`

建议返回：

- `accounts_count`
- `routes_count`
- `active_routes_count`
- `new_routes_24h`
- `inbound_24h`
- `outbound_24h`
- `failed_24h`
- `top_channels`

## 7.2 中心投递日志

- `GET /wunder/admin/bridge_centers/{center_id}/delivery_logs`

查询参数建议：

- `channel`
- `center_account_id`
- `direction`
- `status`
- `route_id`
- `offset`
- `limit`

## 7.3 中心审计日志

- `GET /wunder/admin/bridge_centers/{center_id}/audit_logs`

建议支持检索：

- `action`
- `actor_type`
- `actor_id`
- `route_id`
- `offset`
- `limit`

---

## 8. 调试与预览 API

这组接口很重要，因为舰桥中心是全渠道能力，实际联调时最容易出问题的是身份提取与路由命中。

## 8.1 预览身份提取

- `POST /wunder/admin/bridge_centers/{center_id}/preview_identity`

请求体建议：

```json
{
  "channel": "feishu",
  "account_id": "shared_cs",
  "peer_kind": "group",
  "peer_id": "chat_001",
  "sender_id": "ou_xxx",
  "thread_id": null,
  "display_name": "张三"
}
```

响应建议：

```json
{
  "data": {
    "identity_strategy": "sender_in_peer",
    "external_identity_key": "feishu:shared_cs:chat_001:ou_xxx",
    "normalized_preview": {
      "external_display_name": "张三",
      "external_user_key": "ou_xxx"
    }
  }
}
```

## 8.2 预览路由解析

- `POST /wunder/admin/bridge_centers/{center_id}/preview_route`

用途：

- 给定一组模拟入站元数据，判断会命中手工绑定、舰桥中心还是 owner fallback；
- 如果会自动开户，也返回预估生成的 `wunder_user_id` 与默认预设。

---

## 9. 内部服务接口清单

除了 HTTP API，建议后端内部也明确 service 契约。

## 9.1 `BridgeResolver::resolve_inbound(...)`

输入：

```text
center_account_id
channel
account_id
peer_kind
peer_id
sender_id
thread_id
message_id
payload_meta
```

输出：

```text
route_source(manual|bridge_center|owner_fallback)
route_id?
wunder_user_id
agent_id
created_user(bool)
created_agent(bool)
created_route(bool)
identity_key
```

## 9.2 `BridgeProvisioner::ensure_route(...)`

职责：

- 复用 `provision_external_launch_session(...)`
- 复用 `resolve_or_create_external_embed_agent(...)`
- 原子写入 `bridge_user_routes`

## 9.3 `BridgeLogService::record_delivery(...)`

职责：

- 记录入站/出站日志
- 记录失败原因
- 回填 route/session/message 关联

---

## 10. 明确不做的 API

一期明确不提供：

- `POST /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/change_agent`
- `POST /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/rotate_thread`
- `GET/POST /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/workspace/*`
- `GET/POST /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/cron_jobs/*`
- `PUT /wunder/admin/bridge_centers/{center_id}/routes/{route_id}/system_prompt`

原因不是做不到，而是不应该把舰桥中心演化成第二套用户智能体后台。

---

## 11. 最终建议

API 设计最容易失控的点，是一旦把 bridge 路由当成“特殊用户”，接口就会无限膨胀。

因此必须控制边界：

- 中心 API：管理中心本身
- 共享账号 API：管理共享入口
- 路由 API：管理桥接映射与接入状态
- 日志 API：用于观测与排障
- 深度操作：跳转现有用户页、线程页、工作区页

这样 API 清单才是优雅的、可维护的。
