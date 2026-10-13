---
title: 多用户管理
summary: 舰体的用户、单位与权限边界：不透明令牌与会话轮换、单位树与负责人作用域、按形态分别在线、整数 Credits 配额与 worker card 定义文档。
read_when:
  - 在组织环境部署舰体，需要理解用户/单位/角色怎么划分
  - 排查登录态、挤号、令牌失效或配额不足问题
  - 配置预设智能体与 worker card 的可见范围
source_docs:
  - crates/wunder-runtime/src/services/user_store.rs
  - crates/wunder-runtime/src/api/auth.rs
  - crates/wunder-runtime/src/services/auth_sessions.rs
  - crates/wunder-runtime/src/services/org_units.rs
  - crates/wunder-runtime/src/services/presence/mod.rs
  - crates/wunder-runtime/src/services/user_leveling.rs
  - crates/wunder-runtime/src/services/worker_card_files.rs
  - crates/wunder-runtime/src/orchestrator/quota.rs
  - crates/wunder-core/src/auth.rs
updated_at: 2026-10-13
---

# 多用户管理

多用户治理是**舰体（server）形态**的基础能力；蜂窝与舵机是本地单机形态，不涉及账号体系。本页讲边界与判断依据，面板操作见参考章。

## 身份的三层把关

| 层 | 位置 | 拦谁 |
| --- | --- | --- |
| 管理门 | server 级中间件 | `/wunder/admin/*` 等管理路径：要求 API key 或 Bearer 且账户为管理员；单位负责人按路径开孔 |
| 用户归属 | 逐 handler 的 `resolve_user` + 归属校验 | 用户面：Bearer 解析用户，并校验会话归属（自己的会话/工具/任务） |
| WebSocket | 连接 envelope 携带令牌 | 六条实时通道各自的接入鉴权 |

## 登录态：不透明令牌，不是 JWT

| 凭据 | 形态 | 有效期（默认） |
| --- | --- | --- |
| Access token | `wund_` 前缀随机串，按值查库校验 | 7 天 |
| Refresh token | `wundr_` 前缀随机串 | 30 天 |
| 外部登录一次性码 | `wundc_` 前缀，内存态 | 短时效 |

关键语义：

- **刷新是 OAuth 式族轮换**：一次刷发新 access + 新 refresh，同属一个轮换族；**拿着已吊销的令牌来刷新会被判定为令牌被盗，整族（含在用的 access）立即吊销**。
- **scope 决定谁能持 refresh**：请求头 `x-wunder-session-scope` 标注会话来源；只有 `local_desktop` / `local_cli`（蜂窝、舵机）允许刷新续期，浏览器会话不走这条路。
- **同 scope 挤号**：同一用户在同一 scope 再次登录，旧会话收到 `SESSION_REPLACED` 事件后被断开。判断「为什么另一端的连接掉了」先看是否同 scope 重登。
- 口令用 **Argon2** 哈希存储，明文不落库。

## 在线状态按形态分列

presence 按客户端类型分别 touch：**蜂巢（web）/ 蜂窝（desktop）/ 舵机（cli）** 三路独立，TTL 默认 90 秒、GC 周期 30 秒（下限 10 秒）。用户列表因此拆 `online_web` / `online_desktop` / `online_cli` 三列——「在线」不再是布尔一值，回答「谁在哪种形态上在线」要看对应列。管理出口在 `/wunder/admin/gateway/presence`。

## 单位树与负责人作用域

| 规则 | 事实 |
| --- | --- |
| 播种 | 启动时若单位表为空，从 `config/org_units.json` 模板播种 |
| 深度 | 单位树最深 **4 级** |
| 负责人开孔 | leader 身份仅对 `/wunder/admin/org_units` 与 `/wunder/admin/user_accounts` 放行 |
| 负责人作用域 | 其数据范围 = 自己担任 leader 的单位及其**后代单位**，越出即权限拒绝 |

前端确认只是体验，负责人边界由上述服务端判定兜底。

## 等级与额度是两套东西

**等级（娱乐/成长维度）**：XP = 会话运行秒数取整，上限 `MAX_USER_LEVEL = 200`；升到下一级所需 XP 按 `240 + 28L + 0.9L²` 计算。等级不解锁资源，只反映使用时长。

**额度（治理维度）**：每用户持有**整数 Credits** 账户——`balance` / `granted_total` / `used_total` / `daily_grant`（默认每日发放 1000）。它是发放与扣减的抽象额度，**不是 token 数**：每次向模型服务发起一次请求扣 1 Credit（`account_credits_consumed`），与 token 计量相互独立。余额不足时新一轮请求被配额拒绝并返回剩余额度快照；管理员账户豁免扣减但仍记录请求数。

## worker card：智能体的定义文档，不是线程

术语上必须分清：

| 概念 | 是什么 |
| --- | --- |
| **worker card** | 智能体（用户智能体记录）的**可移植定义文档**，文件后缀 `.worker-card.json`；字段含 `schema_version`、`kind`、`metadata`（agent_id/名称/描述/图标）、`prompt`（解析为 system + extra 两段）、`abilities`、`interaction`、`runtime`、`preset`（预设发布态：revision/status/visible_unit_ids/customizable）、`extensions` |
| **线程/会话** | 智能体被调用后的连续执行单元，一个智能体可同时拥有多个独立任务线程 |

- 预设智能体发布后按 `visible_unit_ids` 控制对哪些单位可见，`customizable` 决定绑定用户能改哪些字段。
- `user_agents.worker_cards_root` **默认为空串**：不配置就没有从磁盘加载的预设卡；预设目录能力是显式配置项，不是出厂自带。
- 审批模式 `suggest` / `auto_edit` / `full_auto`，默认 `full_auto`。

## 数据隔离的落点

- 会话、任务、工作区、工具、渠道绑定等均按用户归属隔离，用户面逐 handler 校验归属。
- 云端文件与执行边界由**沙箱**承担：读/写/执行类操作在每用户容器中执行，只挂本用户目录；路径校验不再负责租户隔离。
- 配额、等级、presence 是账户级；模型配置与预设智能体是单位/全局级。

## 常见误解

- **「token 是自包含 JWT，可以离线解析」**——不是。令牌是不透明随机串，校验靠查库，吊销即时生效。
- **「refresh 丢了可以拿旧的再刷一次」**——旧 refresh 重放会触发整族吊销，正确动作是重新登录。
- **「Credits 就是 token 预算」**——Credits 是整数请求额度，每次模型请求扣 1；token 用量另行计量。
- **「负责人能管所有管理页」**——leader 只在单位与用户账户两条路径放行，且作用域限于后代单位。
- **「worker card 是一种任务线程」**——它是智能体的定义文档；线程是执行单元，两者不同层。
- **「系统自带一批预设智能体卡」**——`worker_cards_root` 缺省为空，未配置即无磁盘预设卡。

## 相关页

- [认证与安全](/docs/zh-CN/ops/auth-and-security/) — 部署侧的密钥与配置
- [舰桥面板指南](/docs/zh-CN/reference/admin-panels/) — 用户管理/单位管理操作
- [Token 与配额](/docs/zh-CN/concepts/quota-and-token-usage/) — 计量口径细节
- [互联互通](/docs/zh-CN/integration/interlink/) — 本地设备接入舰体的治理面
