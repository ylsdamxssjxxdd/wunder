# 舰桥中心 Rust 后端任务拆解（已更新）

## 1. 已落地模块

当前舰桥中心后端已经完成主路径闭环。

已落地模块：

- 管理 API：`src/api/admin_bridge.rs`
- 桥接服务：`src/services/bridge/`
- 渠道接线：`src/channels/service.rs`
- 存储抽象：`src/storage/bridge.rs`
- SQLite / Postgres 落地：`src/storage/sqlite.rs`、`src/storage/postgres.rs`

---

## 2. 已完成能力

### 2.1 数据层

已完成：

- `bridge_centers`
- `bridge_center_accounts`
- `bridge_user_routes`
- `bridge_delivery_logs`
- `bridge_route_audit_logs`

### 2.2 路由主链路

已完成：

- 外部身份提取
- 自动开户
- 自动确保默认预设智能体
- 桥接路由创建与复用
- 入站投递日志记录
- 出站投递日志记录
- 路由最近活动更新

### 2.3 管理 API

已完成：

- 元数据接口
- 支持渠道接口
- 舰桥节点增删改查
- 节点接入渠道增删改查
- 自动分配路由查询与状态治理
- 投递日志查询

### 2.4 管理端页面

已完成：

- 主页改为监控页
- `中心配置` 改为弹窗
- `接入渠道` 改为弹窗
- 管理端接入渠道改为原生 JS 单独实现
- 默认预设智能体来源修正为真实预设列表

---

## 3. 当前剩余工作

仍建议继续补强的部分：

- 后端集成测试：覆盖首包桥接、自动开户、自动确保预设、回消息、日志落库
- 更多渠道联调样例
- 更细的投递失败提示与治理提示

---

## 4. 当前验收标准

满足以下条件即可视为舰桥中心可上线联调：

1. 管理员能新建舰桥节点
2. 管理员能在接入渠道弹窗里配置节点专属渠道账号
3. 外部用户首包进入后自动开户，默认密码 `123456`
4. 系统自动确保该用户拥有节点默认预设智能体
5. 后续消息稳定命中同一桥接路由
6. 回复能从原渠道账号发回
7. 管理端可看到路由和投递日志

---

## 5. 当前校验记录

已通过：

- `cargo check --release`
- `node --check web/modules/bridge-center.js`
- `node --check web/modules/elements.js`
- `node --check web/app.js`
