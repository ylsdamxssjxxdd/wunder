# form-bench

形态级性能采集框架。目标：**一次运行即可测出 README「性能矩阵」所需的全部指标**——
5 种形态 × 6 个指标。

性能矩阵（`README.md` / `README-English.md`）的对象是 5 种形态：

| id | 形态 | 说明 | 载体 |
| --- | --- | --- | --- |
| `hull` | 舰体 Hull | 服务端进程 `wunder-server`（默认 member，共享引擎 `wunder-runtime`） | `crates/wunder-server` |
| `bridge` | 舰桥 Bridge | Web 管理端，由服务端静态托管于 `/` | `web/` |
| `beehive` | 蜂巢 Beehive | Web 用户端（Vue + Vite） | `frontend/` |
| `honeycomb` | 蜂窝 Honeycomb | 原生桌面端（Rust + Slint，单进程内嵌后端） | `frontend-slint/` |
| `helm` | 舵机 Helm | 命令行 TUI | `crates/wunder-cli` |

矩阵的 6 个指标与本框架的 metric key 对应关系：

| 矩阵列 | metric | 单位 | 采集模式 |
| --- | --- | --- | --- |
| 启动速度 | `startup` | ms | tcp / window / cli / web 探活计时 |
| 内存占用 | `memory` | MB | 进程工作集采样 / 页面 JS 堆 |
| CPU 占用 | `cpu` | % | 进程 CPU 时间率 / 渲染主线程 TaskDuration |
| 包体积 | `package_size` | MB | 目录或单文件体积 |
| 并发智能体线程 | `concurrent_agent_threads` | sessions | 逐步加压 + 阈值搜索 |
| 聊天页面性能 | `chat_page_perf` | ms | Playwright 性能用例 |

## 目录结构

```
scripts/form-bench/
├─ form-bench.ps1              # 统一入口（runner）
├─ common.ps1                  # 公共库（UTF-8 IO / 结果封装 / 探针）
├─ collect-package.ps1         # 包体积
├─ collect-startup.ps1         # 启动速度（tcp / window / cli / web）
├─ collect-process.ps1         # 内存 + CPU（本机进程）
├─ collect-web-metrics.mjs     # 内存 / CPU（网页，Playwright + CDP）
├─ collect-concurrency.py      # 并发智能体线程
├─ collect-chat.ps1            # 聊天页面性能（Playwright / slint / none）
├─ summarize.py                # 汇总为 markdown 矩阵 + csv
├─ config/forms.json           # 形态与采集参数（唯一配置源）
└─ README.md
```

产物默认写入 `target/form-bench/`：

- `raw/*.json` — 每个「形态-指标」一个结果封套
- `raw/*.log` — 各采集步骤的原始输出
- `form-bench.json` — 聚合结果
- `form-bench.md` / `form-bench.csv` — 矩阵与明细

## 快速开始

干跑（不执行任何采集，仅打印计划命令，**无需构建、无需服务**）：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\form-bench\form-bench.ps1 -DryRun
```

只跑某几个形态 / 某些指标：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\form-bench\form-bench.ps1 `
  -Forms hull,honeycomb -Metrics package,startup,memory,cpu
```

完整跑（自动拉起 server / 蜂巢 dev server，结束后停止）：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\form-bench\form-bench.ps1 -AutoStart
```

常用参数：`-Forms` `-Metrics` `-OutDir` `-BaseUrl` `-Port` `-DurationSec` `-IntervalMs`
`-StartupRuns` `-StartupWarmup` `-Python` `-Cargo` `-Node` `-AutoStart` `-DryRun` `-SkipSummary`。

> 运行前先构建被测形态（`cargo build --release`、`npm run build --workspace wunder-frontend` 等），
> 否则该形态会产出 `value = null` + `details.error = "target not found"` 的记录，而不会中断整体运行。

## 采集口径

### 启动速度 `startup`
- `tcp`（舰体）：每次冷启动进程，轮询 TCP 端口可连接所需毫秒数；逐次重启，取中位数。
- `window`（蜂窝）：委托 `scripts/desktop-bench/measure-startup.ps1` 测「进程启动 → 主窗口句柄就绪」。
- `cli`（舵机）：启动进程后，轮询 stdout 出现哨兵字符串（默认 `WUNDER_READY`）所需毫秒数。
  ⚠️ 需 `wunder-cli` 支持 `--bench-echo`：打印哨兵后立即退出（见「待办」）。
- `web`（舰桥 / 蜂巢）：委托 `collect-web-metrics.mjs --mode startup`，取导航到
  `DOMContentLoaded` 的中位毫秒数。

### 内存 / CPU
- 本机形态（舰体 / 蜂窝 / 舵机）：`collect-process.ps1` 在采样窗口内对同名进程求和工作集，
  取平均值为内存（MB）；CPU = `cpu_time_ms / (wall_ms × 逻辑核心数) × 100`，即**占整机百分比**。
- 网页形态（舰桥 / 蜂巢）：`collect-web-metrics.mjs`。内存取页面渲染进程 JS 堆
  （`JSHeapUsedSize`，不含浏览器进程本身）；CPU 取渲染主线程 `TaskDuration` 速率
  （占单核百分比）。

### 包体积 `package_size`
- 目录形态：递归求和（MB），并附扩展名分布与 Top10 文件。
- 文件形态：单文件体积（MB）。
- 舰体默认指向 `target/release` 发行目录；如需镜像体积，请在 `config/forms.json` 中把
  舰体 `package.target` 改为镜像导出目录/文件。

### 并发智能体线程 `concurrent_agent_threads`
**负载单位 = 一个智能体线程跑一轮对话（one agent session running one chat round）。**
- 从 `--start` 起按 `--step` 递增到 `--max`，取「成功率 ≥ 阈值 且 p95 ≤ 预算」的**最大并发**为结果。
- 引擎二选一（`collect-concurrency.py --engine`）：
  - `boundary`（默认）：`scripts/runtime_boundary_stress.py` 打 live server。
  - `sim`：`cargo ... backend_sim --concurrency N --report ...` 走进程内引擎仿真。
- 五个形态共享同一引擎，因此该指标按后端能力给出，并记入每个形态行。
- 注意 `config/wunder.yaml` 里 `server.max_active_sessions: 4` 会限制默认并发上限，
  压测前请按需调整或改用不受该限制的场景。

### 聊天页面性能 `chat_page_perf`
- 舰桥 / 蜂巢：跑对应 Playwright 性能用例（`frontend/tests/e2e/*.spec.ts`），取用例耗时（ms）。
- 蜂窝：Slint 原生帧率需在 `frontend-slint` 内加埋点（见「待办」），当前记为 N/A + note。
- 舰体 / 舵机：不适用，记为 N/A。

## 配置（config/forms.json）

唯一配置源。每形态可含：

```
package    { kind: dir|file, target }
startup    { mode: tcp|window|cli|web, ... }
process    { name, mode: native|web, url, args }
concurrency{ supported, mode }
chat       { supported, mode: playwright|slint|none, spec }
```

`defaults` 提供 `serverPort` / `baseUrl` / 采样窗口等兜底值；命令行参数优先级更高。
`processes` 定义可被 `-AutoStart` 拉起的服务（server、beehiveDev）。

> 端口默认对齐 `config/wunder.yaml` 的 `server.host=0.0.0.0 / server.port=18000`
> （`WUNDER_PORT` 可覆盖；sandbox 兜底 9001）。CI 里若用 18001，请 `-Port 18001 -BaseUrl http://127.0.0.1:18001/wunder`。
> 蜂巢 dev server 端口默认 5173，按 `frontend/scripts/dev-server.mjs` 的 `--port` 调整。

## 与既有基准体系的关系

- 本框架**只做形态级横评**，产出矩阵数值，不改动既有基准工具。
- `scripts/benchmark.ps1`、`scripts/runtime_boundary_stress.py`、
  `scripts/run_backend_sim_workflow.py`、`scripts/desktop-bench/*` 均为被复用对象，本框架只调用、不修改。
- 数值验收门槛沿用 `docs/性能标准.md`；基线归档沿用 `docs/性能基线/`
  （命名 `YYYY-MM-DD-<scope>-<topic>.md`）。

## 待办 / 已知限制

- **T-cli-sentinel**：`wunder-cli` 需支持 `--bench-echo`（打印 `WUNDER_READY` 后退出），
  舵机启动速度才能取到有效值；否则记录 `details.error`。
- **T-chat-native**：蜂窝聊天页面性能需在 `frontend-slint` 增加帧耗时埋点，
  否则该格恒为 N/A。
- **T-server-health**：舰体启动当前用 TCP 探活（未依赖 `/health` 路由，项目未见通用 health 路由）；
  若后续新增 `GET /health`，可在 `collect-startup.ps1` 的 tcp 模式加 HTTP 就绪校验。
- **T-package-web**：网页形态包体积目前按源目录计；如需 gzip 后产物体积，
  请在 `package.target` 指向构建产物并另加压缩步骤。
- 并发/聊天两类依赖外部工具与运行环境，**干跑可用于校验命令拼装**；
  真实数值需在具备构建产物与运行服务的机器上执行。