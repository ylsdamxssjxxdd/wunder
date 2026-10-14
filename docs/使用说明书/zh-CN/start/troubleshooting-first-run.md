---
title: 首次运行排查
summary: 各形态第一次启动或部署最常见的失败模式与验证点：蛹启动、舰体部署、蜂巢访问、舵机执行。
updated_at: 2026-10-13
---

# 首次运行排查

按形态分节：验证点、失败现象与回查路径。

## 蛹（desktop）

### 验证点

- `wunder-frontend-slint` 启动后进入工作台，**没有**任何本机监听端口
- `~/.wunder/workspace` 与 `<temp>/config/desktop.settings.json` 已生成
- 设置 → 模型：「测试连接」返回成功
- 发第一条消息能看到工具批次与最终回复

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| 启动页长时间不结束 | `~/.wunder` 权限；工作树扫描耗时（最多 4096 项） | [蛹入门](/docs/zh-CN/start/desktop/) |
| 窗口出现但工作区空 | 工作目录区是否显示「离线」 | [蛹界面](/docs/zh-CN/surfaces/desktop-ui/) |
| 模型请求 401/403 | 设置 → 模型 → API Key、接口地址；「测试连接」重跑 | [配置说明](/docs/zh-CN/capabilities/config/) |
| 运行中再发消息被判「用户忙」 | 会话队列默认开着；关掉会破坏排队条 | [快速开始](/docs/zh-CN/start/quickstart/) |
| 想要 TTS 播放但没有声音 | **蛹 TTS 未实现**，只能配模型；需要 TTS 请用蜂巢 | [蛹界面](/docs/zh-CN/surfaces/desktop-ui/) |
| 找不到「设备互通」设置类目 | 类目未接入界面；可用面只有侧栏云端工作区 + 远端命令审批弹窗 | [界面概览](/docs/zh-CN/surfaces/index/) |
| 状态混乱 | 设置 → 通用 → 一键重置工作状态（保留技能、知识库、长期记忆、配置与工作区文件） | [蛹入门](/docs/zh-CN/start/desktop/) |

## 舰体（server）

### 验证点

- `docker compose ps` 全部 running；`wunder-server`、`wunder-nginx`、`postgres`、`sandbox`、`onlyoffice`、`drawio`、`extra-mcp` 齐
- 健康检查：`GET http://<host>:18000/wunder/i18n` 返回 200
- 舰桥：浏览器打开 `http://<host>:18000/`，看到「4 组 19 项」顶层导航
- 蜂巢：浏览器打开 `http://<host>:18001/`，进入登录页

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| 18000 端口不通 | `WUNDER_HOST` / `WUNDER_PORT` 覆盖；`config/wunder.yaml` 服务器段 | [舰体部署](/docs/zh-CN/start/server/) |
| 舰桥能进但蜂巢 18001 空白 | 蜂巢前端是否构建；网页服务容器是否起来；`docker compose logs wunder-nginx` | [舰体部署](/docs/zh-CN/start/server/) |
| 打开 18000 却是蜂巢的样子 | 两个概念混了——舰体只挂舰桥页面，蜂巢由独立网页服务容器在 18001 下发，不由舰体托管 | [界面概览](/docs/zh-CN/surfaces/index/) |
| 启动时加载 example 配置静默失败、回落默认 | 已知问题；显式指向 `config/wunder.yaml`，或用 `WUNDER_CONFIG_PATH` | [配置说明](/docs/zh-CN/capabilities/config/) |
| 客户端 WS 频繁 `slow_client` | 慢客户端背压（队列上限 2）；先看是否长消息 | [故障排查](/docs/zh-CN/help/troubleshooting/) |
| PostgreSQL 连接失败 | `WUNDER_POSTGRES_DSN` / `WUNDER_POSTGRES_POOL_SIZE`；`postgres` 服务是否 healthy | [舰体部署](/docs/zh-CN/start/server/) |
| 想切回 SQLite | `WUNDER_STORAGE_BACKEND=sqlite` + `WUNDER_SQLITE_DB_PATH=/path/to/wunder.db` | [配置说明](/docs/zh-CN/capabilities/config/) |
| 沙箱模式启动但监听 9001 | 正常：沙箱模式走另一条链路 | [舰体部署](/docs/zh-CN/start/server/) |

## 蜂巢（web）

### 验证点

- 浏览器打开 `http://<host>:18001/`（网页服务容器）
- 登录成功后自动跳到 `/app/chat`
- 输入区「已占用/容量」正常更新
- 时间线里工作过程条目能展开

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| 打开后 `/login` 循环 | 登录令牌是不透明随机串，**不是 JWT**；重放会触发整族吊销 | 认证与安全 |
| 长线程打开慢、渲染卡 | 大数据量线程的渲染优化方案尚未落地，性能瓶颈仍在 | [蜂巢界面](/docs/zh-CN/surfaces/frontend/) |
| WS 断线不恢复 | 断线后有补数端点可补齐；蜂巢只有一条 `/chat/ws` 复用通道 | [蜂巢界面](/docs/zh-CN/surfaces/frontend/) |
| 附件上传失败 | 临时附件上限 200 MB，持久附件 10 MB | [蜂巢界面](/docs/zh-CN/surfaces/frontend/) |

## 舵机（cli）

### 验证点

- 交互式终端里 `wunder-cli` 进入交互界面
- 非 TTY（管道 / CI）自动降级为一次性执行，能退出
- `wunder-cli doctor` 报告绿色
- `wunder-cli exec --json "..."` 每行是合法 JSON

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| 交互界面不启动，直接一次性退出 | stdin/stdout/stderr 是否**全部**是 TTY | [舵机使用](/docs/zh-CN/start/cli/) |
| `~/.wunder` 权限报错 | `WUNDER_HOME` 覆盖；数据目录默认 `~/.wunder` | [舵机使用](/docs/zh-CN/start/cli/) |
| 找不到 `/cron` 命令 | 舵机显式关闭定时任务；无渠道 / 知识库 / 记忆 / 形象管理界面 | [舵机使用](/docs/zh-CN/start/cli/) |
| JSONL 事件缺字段 | 事件名 `thread` / `turn` / `item.*`；对照解析 | [舵机使用](/docs/zh-CN/start/cli/) |
| 想在脚本里跑单工具 | 用 `wunder-cli tool ...` 直跑，不走智能体循环 | [舵机使用](/docs/zh-CN/start/cli/) |

## 通用提醒

- **不要把「停止维护前端壳」理解成「停止维护蛹后端」**：Electron / Tauri 前端壳停止迭代，引擎与蛹后端继续开发。
- 列表、日志、面板都有分页、筛选或限量；如果某个页面一次性渲染大量内容导致卡顿，属于已知的工程缺口，反馈即可。

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [界面概览](/docs/zh-CN/surfaces/index/)
- [蛹入门](/docs/zh-CN/start/desktop/)
- [舰体部署](/docs/zh-CN/start/server/)
- [舵机使用](/docs/zh-CN/start/cli/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
