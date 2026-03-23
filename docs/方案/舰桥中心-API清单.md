# 舰桥中心 API 清单

## 1. 命名约定

- 页面/能力总称：`舰桥中心`
- 单个共享接入单元：`舰桥节点`
- 后端内部主资源：`bridge_center`
- 单个共享渠道挂接：`bridge_center_account`
- 自动分配路由：`bridge_route`

说明：

- 管理端页面已经收敛为“`节点配置 + 接入渠道 + 运行监控`”单模型。
- `共享账号绑定` 不再作为独立产品步骤暴露，而是节点配置中的内嵌列表。
- 当前真实接口以 `src/api/admin_bridge.rs` 与 `docs/API文档.md` 为准。

---

## 2. 已落地接口

### 2.1 元数据与能力探测

- `GET /wunder/admin/bridge/metadata`
- `GET /wunder/admin/bridge/supported_channels`

用途：

- 返回默认开户密码、可选预设智能体、可选单位、已激活渠道账号
- 返回所有可挂入舰桥节点的渠道目录与适配器能力

### 2.2 舰桥节点管理

- `GET /wunder/admin/bridge/centers`
- `POST /wunder/admin/bridge/centers`
- `GET /wunder/admin/bridge/centers/{center_id}`
- `DELETE /wunder/admin/bridge/centers/{center_id}`

说明：

- `POST /centers` 同时承担创建与更新职责
- 节点保存时支持直接提交 `shared_channels[]`
- 管理端页面默认走“节点配置 + 接入渠道一次性保存”

`POST /wunder/admin/bridge/centers` 关键字段：

- `center_id`：可选，传入时表示更新
- `name`
- `code`
- `status`
- `default_preset_agent_name`
- `target_unit_id`
- `default_identity_strategy`
- `username_policy`
- `description`
- `shared_channels[]`

`shared_channels[]` 每项字段：

- `center_account_id`：可选，已有记录更新时携带
- `channel`
- `account_id`
- `enabled`
- `identity_strategy`
- `thread_strategy`
- `default_preset_agent_name_override`
- `status_reason`

### 2.3 节点接入渠道管理

- `GET /wunder/admin/bridge/centers/{center_id}/accounts`
- `POST /wunder/admin/bridge/centers/{center_id}/accounts`
- `PATCH /wunder/admin/bridge/accounts/{center_account_id}`
- `DELETE /wunder/admin/bridge/accounts/{center_account_id}`

说明：

- 这组接口仍然保留，便于脚本化管理与定向调试
- 管理端页面默认不再把它展示成单独步骤
- 同一个 `(channel, account_id)` 只能挂到一个舰桥节点

### 2.4 自动分配路由

- `GET /wunder/admin/bridge/routes`
- `GET /wunder/admin/bridge/routes/{route_id}`
- `PATCH /wunder/admin/bridge/routes/{route_id}`

说明：

- 用于查看外部身份到 wunder 用户/智能体的稳定映射
- 当前只允许桥接级治理动作：`active / paused / blocked / error`
- 不在舰桥中心里直接编辑这些用户自己的线程、定时任务、工作区

### 2.5 投递日志

- `GET /wunder/admin/bridge/delivery_logs`

用途：

- 查询入站/出站投递记录
- 排查 provider 回包失败、共享账号异常、路由命中异常

---

## 3. 当前页面对应关系

### 3.1 舰桥中心页面

左侧：

- 舰桥节点列表

右侧：

- 节点配置
- 接入渠道
- 自动分配路由
- 投递日志

### 3.2 关键交互

1. 管理员在舰桥中心中新建舰桥节点
2. 选择已经在“渠道监控”中配置好的共享账号
3. 指定默认预设智能体
4. 保存舰桥节点
5. 外部用户首包进入后自动开户、自动挂预设智能体、自动建路由
6. 回复沿原共享渠道账号发回

---

## 4. 已明确不做的能力

舰桥中心不负责：

- 直接编辑桥接用户的智能体提示词
- 直接切换桥接用户线程
- 直接管理桥接用户定时任务
- 直接浏览/改写桥接用户工作区

这些操作仍然属于原有用户侧或管理员其他治理页面。

---

## 5. 真实落地状态

已落地：

- 存储层：`bridge_centers / bridge_center_accounts / bridge_user_routes / bridge_delivery_logs / bridge_route_audit_logs`
- 渠道主链路桥接接线
- 首包自动开户
- 自动确保默认预设智能体
- 出站回原共享渠道
- 管理端舰桥中心页面

后续建议继续补：

- 完整集成测试
- 更多管理端校验提示
- 路由/日志联调样例
