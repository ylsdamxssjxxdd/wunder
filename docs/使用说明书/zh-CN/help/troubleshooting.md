---
title: 故障排查
summary: 按“入口 -> 鉴权 -> 配置 -> 依赖 -> 实时通道”顺序排查，可快速定位大部分 Wunder 故障。
read_when:
  - Wunder 跑不起来或行为异常
  - 你已确认不是单纯使用问题
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - config/wunder-example.yaml
---

# 故障排查

建议按链路排查，不建议先全量翻日志。

## 60 秒健康检查

1. 核心入口是否可达：`/wunder`、`/wunder/chat/ws`
2. 鉴权是否匹配：API Key / 用户 Token / 外链鉴权
3. 依赖是否就绪：数据库、sandbox、MCP

## 症状 -> 检查路径

### 1) 接口直接 401 / 403

优先检查：

- 管理接口是否误用用户 Token
- 用户接口是否误用 API Key
- `/a2a`、`/wunder/mcp` 是否携带 API Key
- 外链场景是否配置 `external_auth_key`

### 2) 配置改了没生效

优先检查：

- 实际加载的是 `config/wunder.yaml` 还是示例文件
- 当前实例是否实际读取 `config/wunder.yaml` 或本地运行时 `WUNDER_TEMP/config/wunder.yaml`
- 你改的是 server 配置、extra_mcp 配置，还是前端配置

### 3) 服务启动成功但能力不可用

优先检查依赖：

- PostgreSQL / SQLite 是否可连
- sandbox 是否可达
- extra_mcp 是否启动
- 外部 MCP/A2A 目标是否在线

### 4) 实时状态不更新、看不到中间过程

优先检查：

1. `/wunder/chat/ws` 是否建连成功
2. 是否已回退 SSE
3. `session_id`、`after_event_id` 是否正确

### 5) 工具不出现或无法调用

优先检查：

- 工具是否启用
- MCP / A2A 服务是否 `enabled`
- 当前会话或智能体是否挂载目标工具
- 是否卡在审批态但前端没回传 `approval`

### 6) 附件一直处理中，或者上传后无法发送

优先检查：

- 上传的是否是当前支持的类型：图片、音频、视频、常见文本或 Office 文档
- 文档转换链路是否正常：`POST /wunder/chat/attachments/convert`
- 媒体处理链路是否正常：`POST /wunder/chat/attachments/media/process`
- `temp_dir` 和用户私有工作区是否可写
- 如果是视频重新抽帧失败，检查源文件是否还在，`source_public_path` 是否仍然有效

补充说明：

- 聊天输入区在附件处理完成前会阻止发送，这是正常保护，不是前端卡死
- 长视频会被自动限制总帧数，所以“请求 FPS”和“实际 FPS”可能不同

### 7) 麦克风或截图按钮不可用

优先检查：

- 当前是否为支持对应能力的运行环境。截图依赖 desktop 桥接；浏览器环境不会天然提供桌面截图
- 麦克风权限是否被系统或浏览器拒绝
- 浏览器是否支持 `getUserMedia` / `MediaRecorder` / `AudioContext`
- 桌面桥接是否暴露了对应能力：录音权限请求、截图捕获

补充说明：

- 麦克风不可用时，通常会看到“audio recording is not supported”或权限拒绝提示
- 截图不可用时，前端会直接提示当前环境不支持截图

### 8) 帮助手册空白或打不开

优先检查：

- `/docs/` 静态站点是否已构建并可访问
- 反向代理是否把 `/docs/` 正确暴露到前端所在域名
- 如果是远端 API 模式，`resolveApiBase()` 对应的源站是否真的提供 `/docs/`

补充说明：

- 用户侧“帮助手册”本质上是右栏内嵌 `/docs/?embed=user`
- 如果接口正常但帮助手册空白，优先怀疑文档站没构建、没发布，或代理没转发

## 仍未定位时

回到以下页面继续缩小范围：

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [配置说明](/docs/zh-CN/reference/config/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)

## 提交问题建议附带信息

- 运行形态：`desktop / server / cli`
- 失败入口与时间点
- 关键日志片段
- 是否涉及 WS、SSE、渠道、MCP、A2A
