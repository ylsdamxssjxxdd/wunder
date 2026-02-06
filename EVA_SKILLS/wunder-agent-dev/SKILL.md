---
名称: wunder智能体开发
描述: 面向 wunder 项目的智能体开发与联调指南。适用于新增/调整 Rust 后端能力（网关、渠道、工具、会话/线程、WS/SSE、存储与配置）、补齐文档、编写回归测试与运行 docker compose 验证等场景。
---

# wunder 智能体开发指南

## 快速目标
- 读懂主链路与系统边界，定位改动范围
- 按 AGENTS.md 规则实现功能、补齐文档与迭代记录
- 用可重复脚本完成联调与回路验证

## 必读前置
- 先读仓库根目录 `AGENTS.md`，严格遵守其中的约束与流程
- 只维护 `src/` 下 Rust 代码；`app/` 目录无需维护
- `data/` 目录是临时目录，不要存放产物

## 核心主链路
```
user_id → agent_id → session_id → agent_loop → WS/SSE 事件
```
- 每个智能体应用有一个主线程（主会话）
- 渠道/网关的接入最终也要落回这条链路

## 标准开发流程
1) 需求澄清
   - 明确涉及：网关 / 渠道 / 工具 / 会话 / 存储 / 配置 / 文档
2) 定位模块
   - API：`src/api/*`
   - 网关：`src/gateway/*`
   - 渠道：`src/channels/*`
   - 调度：`src/orchestrator/*`
   - 配置：`src/core/config.rs` + `config/wunder.yaml`
   - 存储：`src/storage/*`
3) 实现改动
   - 尽量合并 if（clippy::collapsible_if）
   - 能用方法引用就别用闭包（clippy::redundant_closure_for_method_calls）
   - `format!` 内联 `{var}`
4) 同步文档
   - 新增/变更 API → 更新 `docs/API文档.md`
   - 结构变化 → 更新 `docs/设计方案.md`/`docs/系统介绍.md`
5) 记录迭代
   - 使用脚本：`python scripts/update_feature_log.py --type <类型> --scope <范围> "内容"`
6) 格式化/验证
   - Rust：优先 `cargo fmt`，必要时 `cargo clippy`
7) 联调与回归
   - 用脚本或 curl/WS 验证主链路和回路

## 关键规范
- 不要创建 git 分支或提交
- 不要大范围搜索 `frontend/` 根目录（噪音极大）
- 前端不要用 `backdrop-filter`
- 文本统一 UTF-8，避免乱码
- Token 统计是“上下文占用量”，不是总消耗量

## 目录速查
- `src/api/`：HTTP/WS 路由
- `src/gateway/`：网关控制面与节点接入
- `src/channels/`：多渠道接入
- `src/orchestrator/`：主执行链路、工具调用、事件流
- `src/storage/`：SQLite/Postgres 存储
- `config/wunder.yaml`：基础配置
- `data/config/wunder.override.yaml`：管理端覆盖配置
- `docs/`：设计/API/系统/方案文档

## 高频任务模板

### A. 新增 HTTP API
1) `src/api/*` 增加路由与 handler
2) 加鉴权与输入校验
3) 若涉及存储：补充 `storage` trait + sqlite/postgres
4) 更新 `docs/API文档.md`
5) 记录迭代

### B. 新增 WS 控制面或协议字段
1) `src/api/*_ws.rs` 增加协议字段
2) 更新事件与协议说明（`docs/API文档.md`、`docs/WebSocket-Transport.md`）
3) 需要回路测试脚本

### C. 新增内置工具
1) `src/services/tools.rs` 增加 ToolSpec
2) 补别名映射与执行分支
3) 更新 `config/wunder.yaml` 工具列表
4) 更新 `config/i18n.messages.json` 文案
5) 更新 `docs/API文档.md`

### D. 新增存储表/记录
1) `src/storage/mod.rs` 增加 trait
2) `src/storage/sqlite.rs` + `postgres.rs`
3) 必要时加迁移逻辑
4) 更新文档（数据模型/接口）

### E. 新增配置项
1) `src/core/config.rs` 增加字段
2) `config/wunder.yaml` + `docs/API文档.md`/`docs/系统介绍.md`
3) 若管理端可改，补 override 支持

## 联调与验证

### 1) 网关节点回路测试
- 脚本：`EVA_SKILLS/wunder-agent-dev/scripts/gateway_smoke_test.py`
- 作用：
  - 创建 node token → 节点 WS 连接 → 触发 `admin/gateway/invoke` → 验证回包
- 依赖：`aiohttp`、`websockets`（当前环境已可用）
- 环境变量：`WUNDER_BASE_URL`、`WUNDER_API_KEY`、`WUNDER_NODE_ID`、`WUNDER_GATEWAY_WS`
- Windows 建议：`set PYTHONIOENCODING=utf-8`

### 2) SSE/WS 调试建议
- `/wunder` + `debug_payload=true` 拿完整请求体
- 关注事件：`llm_request`、`tool_call`、`tool_result`、`final`

## 经验技巧
- LLM 输出可能“伪造工具结果”，关键回路请直接走管理端或脚本触发
- 网关 WS 鉴权需在握手阶段传 `Authorization: Bearer <token>`，`connect.params.auth.token` 只做逻辑校验
- `wunder.override.yaml` 会覆盖基础配置，联调前先确认生效值
- 编码乱码先用 ASCII 对照用例定位，别用乱码样本做结论

## 脚本说明
- `scripts/update_feature_log.py`：写入 `docs/功能迭代.md`
- `EVA_SKILLS/wunder-agent-dev/scripts/gateway_smoke_test.py`：网关节点回路测试

## 交付自检清单
- [ ] 变更范围与主链路一致
- [ ] 文档与 API 同步
- [ ] 迭代记录已写入
- [ ] 格式化与基础回归通过
- [ ] 联调脚本跑通
