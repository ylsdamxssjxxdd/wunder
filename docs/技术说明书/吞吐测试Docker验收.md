# 吞吐测试 Docker x86 验收

该环境通过 `linux/amd64` 服务、独立 PostgreSQL 卷和本地 HTTP 模型夹具验证管理员吞吐页面。只调用本地模拟模型；运行配置、工作区、性能摘要和验收结果位于 `temp_dir/throughput-docker/`，不读取日常服务配置。需已有 `wunder-x86:latest`、`postgres:16` 镜像，以及仓库 x86 离线 Cargo 缓存。

在仓库根目录运行：

```powershell
python scripts/prepare_throughput_docker.py
docker compose -p wunder-throughput -f packaging/docker/docker-compose-throughput.yml up -d
docker compose -p wunder-throughput -f packaging/docker/docker-compose-throughput.yml logs -f server
```

服务启动时以 Release、8 编译任务构建。首次构建完成后访问 `http://127.0.0.1:18000`，使用隔离库初始化的管理员账号登录，进入“调试 → 吞吐量测试”。接口测试使用准备脚本生成的本地随机密钥；浏览器测试使用隔离库默认管理员，也可通过 `WUNDER_TEST_ADMIN_USER` / `WUNDER_TEST_ADMIN_PASSWORD` 设置。密钥只写本地文件，不输出到测试日志。

```powershell
node tests/throughput-admin-smoke.mjs
node tests/throughput-docker-acceptance.mjs
node tests/throughput-docker-browser.mjs
node tests/virtual-model-browser.mjs
```

需 Node.js 22+ 和已有 Playwright Chromium。第二项执行完整模型请求，包括快、中、慢三档，约三分钟；第三项复查现有结果、正常登录、导航及 WebSocket，无需再次生成；第四项验证模型速度选择、保存和切换，并恢复默认快档。首次构建较慢时，先等待服务就绪再执行验收脚本。

重启服务后执行 `node tests/throughput-docker-acceptance.mjs --verify-history`，逐条核对历史摘要与日志表数量，确认没有恢复旧请求。重启会先检查源码并在有变化时重新构建。

## 模型与指标口径

- 内置虚拟模型：按当前消息估算输入 Token，快 / 中 / 慢档预处理分别为 2000 / 500 / 100 Token/s，思考和正文生成分别为 200 / 50 / 10 Token/s，默认快；使用实际时钟和共享流式指标解析器。先思考（目标输出的 1/4）再生成正文，两者合计恰好等于指定输出数量。吞吐模拟不读取虚拟回放日志，不创建线程。
- HTTP 夹具：通过 Chat Completions SSE 返回思考与正文流，使用快档速度。预处理期间发送角色帧和心跳，验证这些事件不被误认为首个输出 Token。
- 另设提前结束、缺失 usage、断流、拒绝参数四种夹具模式，验证未达标与失败不会伪装为成功。
- 输入档位是估算，usage 与首字耗时用于算实测输入速度；网络和定时器开销使结果略低于配置值。生成速度排除首字等待，思考作为输出的子项单列。
- 内置模拟记录带 `simulated: true` 及当次 `simulation_speed`，历史按档位分组；旧摘要归入 legacy。HTTP 夹具对服务表现为普通 API，记录不带此标记；此验收环境的全部模型均为模拟服务。

## 验收范围与记录

三档速度与思考阶段的 Linux x86_64 Release + PostgreSQL 验收记录：

| 路径 / 档位 | 输入 | 输出目标 / 实际（其中思考） | 首字延迟 | 输入 / 首字耗时 | 生成速度 |
| --- | --- | --- | --- | --- | --- |
| 内置快档 | 1k | 1024 / 1024（256） | 514.9 ms | 1990.8 Token/s | 200.00 Token/s |
| 内置快档 | 8k | 1024 / 1024（256） | 4098.7 ms | 1998.9 Token/s | 200.01 Token/s |
| 内置中档 | 1k | 1024 / 1024（256） | 2052.4 ms | 499.4 Token/s | 50.00 Token/s |
| 内置慢档 | 1k | 1024 / 1024（256） | 10253.0 ms | 100.0 Token/s | 10.00 Token/s |
| HTTP 快档 | 2k | 2048 / 2048（512） | 1035.2 ms | 1979.3 Token/s | 199.99 Token/s |

夹具核验实际收到 `min_tokens=2048` 与 `ignore_eos=true`；异常模式、运行冲突、无鉴权拒绝、票据单次使用、WebSocket 序号、两条路径的 1m 预处理取消均通过。完整验收期间用户数不变，会话、消息历史、监控会话与流事件数量始终为 0。1m 只验证预处理期间取消，未等待完整生成；真实供应商和 GPU 推理速度不属于此次模拟验收范围。

服务重启后恢复 9 条摘要，未恢复旧请求，表计数保持不变；正常管理员登录、导航返回、WebSocket 重连与历史详情曲线检查通过。浮点指标按 12 位有效数字比较，允许 JSON 解码的末位舍入差异。

Windows x64 MSVC Release 的 runtime 检查（同时启用 SQLite/PostgreSQL）无错误或告警；流解析与三档测量 8 项、虚拟模型与回放 14 项、普通虚拟调用 1 项定向测试通过。Linux x86_64 Release 服务构建通过。端到端数据库验证使用 PostgreSQL；SQLite 覆盖普通虚拟调用回归，未另起完整 SQLite 服务进行浏览器验收。管理员页面的独立 Playwright 回归覆盖长度档位、提交参数、停止、错误提示、历史选择、导出、中英文和窄屏布局；模型配置真实浏览器验证三档选择、保存、切换回显与默认快档。

## 并发语义

内置模拟没有独立的并发配额，每个请求按独立时钟模拟对应速度，不共享 GPU，也不按并发数平分速率。因此并发吞吐线性增长只验证定时器与流式处理，不代表真实推理容量。普通线程仍遵循 `server.max_active_sessions`（默认 300）和线程调度；部分管理员调试入口可绕过普通队列，依然受 CPU、内存与连接数限制。吞吐管理页面支持自定义并发数，并聚合批次结果。

`throughput_model` 集成测试直接使用共享预处理与生成执行层，在 8 个 Tokio 工作线程下依次测试 1 / 8 / 32 并发。每个请求为 1024 输入、256 思考 + 768 正文，核对顺序、完整性、每请求速度与聚合速度，不创建用户或线程日志：

```powershell
docker exec wunder-throughput-server-1 cargo test --release -j 8 -p wunder-runtime --features postgres-storage,mcp,host-metrics,web-fetch --test throughput_model -- --nocapture
```

这是模型执行层并发验收，不是 HTTP 网关或完整智能体调度的容量上限测试。

Docker Linux x86_64 Release 实测（每请求 1024 输入、1024 总输出，含 256 思考）：

| 并发 | 平均首字延迟 | 每请求生成速度 | 聚合端到端吞吐 |
| --- | --- | --- | --- |
| 1 | 513.88 ms | 200.01 Token/s | 181.91 Token/s |
| 8 | 513.87 ms | 200.01 Token/s | 1455.35 Token/s |
| 32 | 514.58 ms | 199.99 Token/s | 5820.32 Token/s |

全部请求的思考 / 正文顺序、长度与 usage 校验通过。此次最高验证 32 并发，没有测出容量极限，不代表可无限并发。

结果与截图保存在 `temp_dir/throughput-docker/results.json`、`page.png` 和 `history.png`。历史保存在隔离运行目录下的 `config/data/throughput/scenarios-v2.json`，仅含最近 50 次摘要。不要将本地密钥、运行配置或请求内容加入版本控制。

停止环境且保留数据：

```powershell
docker compose -p wunder-throughput -f packaging/docker/docker-compose-throughput.yml down
```


## 模拟能力与工具调用验收

`node tests/virtual-model-browser.mjs` 验证三档速度、视觉/听觉、思考和工具开关、媒体计量、工具调用方式的保存与切换恢复，并断言工具名和 JSON 参数输入框不存在，结束后恢复快档和默认能力。

`node tests/virtual-model-docker.mjs` 上传临时结构化/文本调用日志，使用临时模型配置，结束后删除测试日志、移除配置并恢复工具开关。Linux x86_64 Release + PostgreSQL 实测通过：

- 结构化和文本日志分别覆盖 `function_call`、`tool_call`、`freeform_call` 的 Chat / Responses 组合。只读工具由真实执行器完成，每次仅执行一次，第二轮回放日志中的最终回复。原生调用第一轮以 `tool_calls` 结束，文本协议以 `stop` 结束。
- 工具能力关闭时返回 `unsupported_tools`，无工具执行事件；非法媒体 token 配置在管理员保存接口返回 400。
- 关闭思考后，1024 token 输出完整，reasoning 为 0，快档生成速度在 200 Token/s 的 5% 范围内。
- 请求 1024 输出但模型最大输出为 512 时，吞吐状态为 `error`，错误包含 `max_tokens_exceeded`。
- 1k 档提示词实际估算为 1025 token，配置窗口 2048、输出 1024 时，虽然档位预校验通过，请求层仍返回 `context_length_exceeded`。
- 连续启动需等待上一条摘要落盘；脚本只对保存窗口的 409 做短暂等待重试，不绕过任务互斥。

这里的普通工具调用验收会产生正常调试线程；吞吐请求仍不产生线程日志。媒体能力不做内容识别，按配置估算 token。文本边界、媒体开关与计量、UTF-8 输出截断、工具参数计量和回放限制另由共享运行时定向测试覆盖。

本次定向验证：Windows MSVC Release 模拟/编排相关 24 项通过，吞吐适配 9 项通过；Windows SQLite + PostgreSQL 配置检查和 Linux PostgreSQL 服务构建通过。管理端吞吐脚本、模拟配置浏览器脚本与 Docker 能力验收通过。
