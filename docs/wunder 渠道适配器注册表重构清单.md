# wunder 渠道适配器注册表重构清单

## 0. 目标与结果

- 目标：将渠道接入从“多处硬编码改动”重构为“注册表驱动 + 元数据驱动”，显著降低新增渠道成本。
- 结果：新增渠道时，默认只需实现适配器并注册；管理端与用户端配置能力可复用统一目录，不再散落在多个 `if/else` 分支。

### 0.1 当前落地状态（2026-03-02）

- ✅ `src/channels/adapter.rs`：已落地统一适配器抽象（入站验签/解析、出站发送、健康检查）。
- ✅ `src/channels/registry.rs`：已落地运行时注册表与默认渠道装配（`whatsapp/feishu/qqbot/wechat/wechat_mp`）。
- ✅ `src/channels/catalog.rs`：已落地渠道目录元数据与查询函数。
- ✅ `src/channels/service.rs`：`deliver_outbox_record` 已切换为“注册表优先 + `outbound_url` 回退”。
- ✅ `src/api/channel.rs`：`/wunder/channel/{provider}/webhook` 已接入适配器验签与解析钩子。
- ✅ `src/api/user_channels.rs`：`supported_channels` 已切换为 catalog 驱动并返回扩展字段。
- ✅ `docs/设计方案.md`、`docs/API文档.md`、`docs/系统介绍.md` 已同步更新。
- ✅ 构建验证：`cargo check` 通过；注册表单测（`cargo test registry --lib`）通过。

## 1. 当前现状（重构基线）

- 渠道核心链路：`src/channels/service.rs`（`ChannelHub` 负责入站处理、会话绑定、出站投递）。
- 通用入站入口：`src/api/channel.rs` 的 `/wunder/channel/{provider}/webhook`。
- 当前痛点：
  - 出站分发依赖渠道名硬编码分支：`src/channels/service.rs`（`deliver_outbox_record`）。
  - 用户侧支持渠道列表为静态常量：`src/api/user_channels.rs`（`SUPPORTED_USER_CHANNELS`）。
  - 新增渠道常需要同时改 API、服务、配置、文档，多点修改易漏。

## 2. 重构范围与非目标

### 2.1 范围（本次要做）

- 建立渠道适配器统一抽象（入站解析、出站发送、可选验签/探活）。
- 建立运行时适配器注册表（按 `channel` 名称注册/查找）。
- 建立渠道元数据目录（用于 admin/user 展示、校验、模板）。
- 将现有已支持渠道迁移到注册表路径（whatsapp/feishu/qqbot/wechat/wechat_mp）。

### 2.2 非目标（本次不做）

- 不重写现有渠道业务语义（保持现网行为一致）。
- 不改动数据库表结构（优先复用现有 `channel_*` 表）。
- 不一次性扩展所有渠道 SDK（先打通架构与模板）。

## 3. 目标架构

### 3.1 核心抽象

建议新增：`src/channels/adapter.rs`

- `ChannelAdapter`（每个渠道实现）
  - `channel() -> &'static str`
  - `parse_inbound(...) -> Result<Vec<ChannelMessage>>`（可选，走通用 webhook 时可不实现）
  - `send_outbound(...) -> Result<()>`
  - `verify_inbound(...) -> Result<()>`（可选）
  - `health_check(...) -> Result<...>`（可选）

- `ChannelAdapterRegistry`
  - `register(adapter)`
  - `get(channel)`
  - `list()`

### 3.2 元数据目录

建议新增：`src/channels/catalog.rs`

- `ChannelCatalogItem`：`channel`、`display_name`、`auth_fields`、`supports`、`webhook_mode`、`docs`。
- 用于：
  - admin 侧渠道配置表单生成；
  - user 侧 `supported_channels` 动态返回；
  - 新渠道接入时避免重复定义。

### 3.3 执行路径

- 入站：`/wunder/channel/{provider}/webhook` -> `ChannelHub::handle_inbound`（保持主链路）
- 出站：`ChannelHub::deliver_outbox_record` 先查注册表，命中适配器则调用；未命中回落通用 `outbound_url`。
- 验签：支持“路由层验签”或“适配器验签”，统一错误码与日志格式。

## 4. 详细改造清单（按模块）

- `src/channels/adapter.rs`（新增）
  - 定义 `ChannelAdapter` trait、上下文结构、错误类型。
- `src/channels/registry.rs`（新增）
  - 注册表实现，支持静态注册与启动时装配。
- `src/channels/catalog.rs`（新增）
  - 渠道元数据统一来源。
- `src/channels/mod.rs`
  - 导出 `adapter/registry/catalog`。
- `src/channels/service.rs`
  - 用注册表替换 `deliver_outbox_record` 的渠道硬编码主分支。
  - 保留通用 `outbound_url` 回退。
- `src/api/channel.rs`
  - 保持现有专用 webhook；通用入口优先使用注册表能力。
- `src/api/user_channels.rs`
  - `SUPPORTED_USER_CHANNELS` 改为由 `catalog` 提供（允许按能力过滤）。
- `src/api/admin.rs`
  - 渠道账号配置表单和校验逐步切换到 `catalog`。
- `docs/API文档.md`
  - 更新“支持渠道来源”与“新渠道接入步骤”。

## 5. 分阶段节点（含验收标准）

### M0 - 基线冻结（0.5 天）

- 输出当前渠道能力清单（入站、出站、验签、长连接）。
- 验收：形成一页迁移映射表（旧分支 -> 新适配器）。

### M1 - 抽象与注册表落地（1 天）

- 新增 `ChannelAdapter` trait、注册表、基础单测。
- 验收：`cargo check` 通过；注册表单测覆盖注册/查询/重复注册行为。

### M2 - 出站链路切换（1.5 天）

- 将 `deliver_outbox_record` 改为“注册表优先 + 通用回退”。
- 先迁移一个渠道（建议 `feishu`）做样板。
- 验收：样板渠道出站回归通过；未迁移渠道行为不变。

### M3 - 现有渠道批量迁移（2 天）

- 迁移 `whatsapp_cloud`、`qqbot`、`wechat`、`wechat_mp`。
- 验收：5 个渠道全部走注册表；旧硬编码分支删除或仅保留临时兼容注释。

### M4 - 目录驱动管理面（1 天）

- 新增 `catalog` 并替换 user/admin 侧静态渠道列表来源。
- 验收：`/wunder/channels/accounts` 与 admin 渠道接口返回一致；前端无需额外 hardcode。

### M5 - 入站能力收敛（1 天）

- 通用 webhook 支持调用适配器可选验签/解析钩子。
- 验收：通用渠道可选启用适配器验签；错误语义统一。

### M6 - 文档与模板收尾（0.5 天）

- 提供“新增渠道模板”与接入 SOP。
- 更新 API/设计文档与功能迭代记录。
- 验收：新同学按模板可在 0.5~1 天接入内部 webhook 渠道。

## 6. 测试清单

- 单元测试
  - 注册表：注册、覆盖、查询失败。
  - 适配器：入站解析、出站 payload、签名校验。
- 集成测试
  - 入站 -> 会话绑定 -> 编排 -> 出站全链路。
  - outbox 重试、失败回退、监控事件完整性。
- 回归测试
  - 现有 5 条渠道链路行为一致（消息、审批、/stop、重试、TTS）。

## 7. 风险与回滚

- 风险
  - 迁移阶段行为偏差（尤其签名与账号选择逻辑）。
  - 用户侧支持渠道列表来源切换引发展示差异。
- 控制
  - 保留“注册表开关”（配置项）与通用回退路径。
  - 每迁移一个渠道即做灰度回归。
- 回滚
  - 可临时切回旧出站分支（保留一个版本周期）。

## 8. 新渠道接入 SOP（重构完成后）

### 8.1 快速接入（内部 webhook 渠道）

1. 新建 `src/channels/<channel>.rs`，实现 `send_outbound`（可选 `verify_inbound`）。
2. 在 `registry` 注册适配器。
3. 在 `catalog` 增加元数据项。
4. 通过 admin 接口创建 `channel/account`。
5. 用 `/wunder/channel/{provider}/webhook` 联调。

### 8.2 深度接入（平台 SDK/长连接）

1. 在适配器内增加长连接任务或 SDK 客户端。
2. 复用 `ChannelHub::handle_inbound` 作为统一主链路入口。
3. 增加专用健康检查与断线重连策略。
4. 补充专用集成测试和限流配置。

## 9. 完成判定（DoD）

- 新增一个“内部渠道”时，核心代码改动点不超过 3 处（适配器、注册、目录）。
- 现有渠道无功能回退，`cargo check` 无错误。
- 文档、API 说明、功能迭代记录同步完成。
