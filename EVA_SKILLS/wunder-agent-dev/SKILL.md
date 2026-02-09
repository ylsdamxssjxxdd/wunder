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

### 3) 智能体容器协作/隔离回归
- 脚本：`EVA_SKILLS/wunder-agent-dev/scripts/agent_sandbox_collab_check.py`
- 作用：
  - 校验预置智能体的 `sandbox_container_id` 布局（文稿校对/数据分析/科学绘图/政策分析/公文写作 = 2~6）
  - 自动创建临时智能体，验证“同容器共享文件区、跨容器隔离文件区”
- 环境变量：`WUNDER_BASE_URL`、`WUNDER_USER_TOKEN`（可选，优先）或 `WUNDER_USERNAME` + `WUNDER_PASSWORD`

## 经验技巧
- LLM 输出可能“伪造工具结果”，关键回路请直接走管理端或脚本触发
- 网关 WS 鉴权需在握手阶段传 `Authorization: Bearer <token>`，`connect.params.auth.token` 只做逻辑校验
- `wunder.override.yaml` 会覆盖基础配置，联调前先确认生效值
- 编码乱码先用 ASCII 对照用例定位，别用乱码样本做结论
- 工作区路由现在优先按 `user_id + sandbox_container_id`，仅在无可用容器信息时回退到历史路由
- 删除智能体时要特别注意：同容器可被多个智能体共享，不应默认清理整个容器文件区
- 预置智能体容器改造建议用“一次性 meta 标记 + 仅修正默认容器值”的策略，避免覆盖用户自定义设置
- API 异常排障建议优先看 `error.code` / `error.hint` / `error.trace_id`，并在前端提供 trace_id 一键复制

## 脚本说明
- `scripts/update_feature_log.py`：写入 `docs/功能迭代.md`
- `EVA_SKILLS/wunder-agent-dev/scripts/gateway_smoke_test.py`：网关节点回路测试
- `EVA_SKILLS/wunder-agent-dev/scripts/admin_nav_sanity.py`：管理端导航与 i18n 一致性检查
- `EVA_SKILLS/wunder-agent-dev/scripts/agent_sandbox_collab_check.py`：智能体容器共享/隔离回归检查

## 交付自检清单
- [ ] 变更范围与主链路一致
- [ ] 文档与 API 同步
- [ ] 迭代记录已写入
- [ ] 格式化与基础回归通过
- [ ] 联调脚本跑通


## 近期补充经验（网关与前端重构）
- 网关联调建议固定顺序：先确认网关开关与鉴权配置，再重启 docker compose，最后跑节点回路脚本。
- 渠道入站建议统一走“解析 -> 归一化 -> 绑定路由 -> 主链路投递”，避免每个渠道重复造轮子。
- 未命中渠道绑定时，建议默认回落到通用应用主线程，避免消息被吞。
- 管理端导航改造为“系统/智能体/调试/文档”后，工具入口建议收敛到单入口，再用快捷按钮跳转子模块。
- 多面板共用同一导航高亮时，需要在切面板逻辑里先清空全部 active，再设置当前 active。
- 导航改造后的高频漏改点是四个文件：web/index.html、web/modules/elements.js、web/app.js、web/modules/i18n.js。
- i18n 新键必须同步中英文，至少覆盖：分组名、面板名、提示语。
- CLI 环境下不要用 shell 调 apply_patch，直接用编辑器或脚本改文件。

### 管理端导航一致性检查脚本
- 新增脚本：EVA_SKILLS/wunder-agent-dev/scripts/admin_nav_sanity.py
- 用途：快速检查导航分组、工具快捷入口、JS 绑定和 i18n 键是否一致。
- 运行方式：python3 EVA_SKILLS/wunder-agent-dev/scripts/admin_nav_sanity.py

## 近期补充经验（文档/图表/DOCX）
- 文档重构后恢复图表，优先采用“回填到对应章节”而非统一放附录，避免结构与图脱节。
- Mermaid 兼容性优先：subgraph 标题和节点文案尽量加引号，避免括号或特殊字符导致旧渲染器报错。
- 图表维护建议固定流程：先改 docs/diagrams/system-intro 下的 mmd 源文件，再批量渲染 svg 和 png，最后同步到 web/docs/diagrams/system-intro。
- web/docs/paper.md 的图片链接不要带查询参数（例如 ?v=时间戳），否则 pandoc 资源解析可能找不到本地图片。
- 生成论文 DOCX 时优先使用 EVA_SKILLS/公文写作/scripts/convert_markdown_to_docx.py 并开启 use-pandoc，确保 Markdown 图片真正内嵌。
- 生成后做嵌图验收：检查 docx 压缩包中 word/media 文件数量，并核对 word/document.xml 中 drawing 计数，确认图片没有丢失。
- 若 mermaid-cli 提示缺少浏览器，使用 puppeteer 配置文件指定本机 chrome-headless-shell.exe 路径可稳定渲染。
- 文档和图表改动完成后，按仓库规范及时写入 docs/功能迭代.md，避免经验沉没。
