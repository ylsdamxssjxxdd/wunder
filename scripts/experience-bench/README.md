# 蜂巢与舵机外部体验测试

## 服务并发容量

`measure-capacity.py` 使用新的隔离 Compose 项目，生成 2000 个不同合成账号和令牌，创建独立线程，保持 2000 路 WebSocket，再逐级同时提交任务。生产准入仍为 300 个运行槽，超出部分必须排队；不是把 2000 个任务全部放入模型执行。模型使用异步 SSE 夹具，64 段、每段 8ms，无真实推理和计费。账号由 SQL 种子生成，因此不测试 2000 次密码登录散列峰值。

```powershell
python scripts/experience-bench/measure-capacity.py --server-binary target/x86/release/wunder-server --output temp_dir/capacity-run --levels 100,500,2000
```

宿主驱动 Python 3.8+；镜像内并发驱动要求 Python 3.11+、`aiohttp`。测试不下载镜像、不连接已有业务库，退出清理自己的 Compose 项目及令牌文件。驱动使用 opt-in profile，不随 `compose up` 提前启动。`capacity.json` 保存在线结果、各级任务完成/错误、排队数、舰桥探针与持久化核对；`report.json` 另保留有界容器资源样本和最后 2000 行服务日志的分类计数，不导出原始日志或凭据。失败即停止扩大负载，不能把缺少的 2000 级结果当作通过。排队线程使用有界显示快照及必要补水，不强行要求它走实时 token 通道；最终每个线程仍核对用户输入、完整回复与终态。短时合成压测不是生产 ARM、真实模型或长期稳定性的 SLA。

方案与指标：[蜂巢](../../docs/蜂巢体验测试方案.md)、[舵机](../../docs/舵机体验测试方案.md)。本目录只提供外部驱动，不给产品增加测试入口。

在仓库根目录执行，Python 3.8+：

```powershell
python -m pip install -r scripts/experience-bench/requirements.txt
python -m unittest discover -s tests -p test_external_experience_benchmarks.py -v
```

蜂巢使用 Linux release 舰体二进制、`wunder-x86:latest` 运行环境镜像和 `postgres:16`。镜像需事先准备；测试不会拉取或构建镜像，构建不计入耗时。

```powershell
python scripts/experience-bench/measure-hive.py --server-binary target/x86/release/wunder-server --output temp_dir/hive-smoke --profile smoke
```

驱动自行调用 `docker compose up`，使用独立项目、全新数据库卷、随机本机端口、随机合成账号、受控 SSE 模型和隔离目录。正常或失败退出时清理该项目的容器、卷和生成的凭证；已有 Compose 服务不会被重启。报告不保存原始响应、令牌或容器环境。强制终止 Python 时，需由操作者根据本次创建的 `wunder-exp-*` 项目清理遗留资源，不能批量删除其他项目。

舵机使用当前源码构建的正式 CLI，不使用真实模型配置：

```powershell
cargo build --offline --release -p wunder-cli --bin wunder-cli
python scripts/experience-bench/measure-cli.py --binary target/release/wunder-cli.exe --output temp_dir/cli-smoke --profile smoke
```

Linux 将 CLI 文件名改为 `wunder-cli`。`WUNDER_HOME`、工作目录、配置与数据库均在报告目录下新建；不传入仓库配置。测试会运行正式 `exec`、`resume`、`stress-thread`、`trajectory` 等命令。

`smoke` 每场景运行 1 次，检查链路；`daily` 预热 1 次、采样 10 次；`release` 预热 2 次、采样 30 次。`--cases` 可逗号分隔选场景。默认大线程为 100 用户轮次 × 每轮 100 模型请求，另外包含工具动作；实际数量在 `fixture` 中记录。此负载与蛹的严格 100 模型动作负载不同。

蜂巢要求 PostgreSQL canonical 写入器生成完整工具负载，并核对最终回复身份、模型请求数、双向聊天翻页和权威正文。`fixture_contract` 变化时拒绝与旧基线比较。`storage_bytes=null` 表示尚未测量 PostgreSQL 物理字节，不表示空线程。

服务存储改动可先运行以下隔离验收，使用已经构建的 Linux 二进制，不编译或重启现有服务：

```powershell
python scripts/experience-bench/measure-hive.py --server-binary target/x86/release/wunder-server --output temp_dir/hive-storage-smoke --profile smoke --user-rounds 30 --model-rounds 30 --idle-seconds 600
```

`storage_audit` 对本次独立 PostgreSQL 卷检查 canonical ordinal 连续性、展示水位不领先及 admission/指标/资源表，记录 canonical 逻辑字节、数据库字节和十个相关关系的物理字节；同时限定扫描该测试的应用数据目录，发现 SQLite、会话 JSONL 或本地 canonical 所有权标记即失败。工作区中的用户文件不属于该扫描范围。`restart_recovery` 只重启本次独立 Compose 项目的服务容器，核对重启前后 canonical 记录数量、ordinal 与内容摘要一致，再继续发送一轮。此驱动验证服务 HTTP/WebSocket 与持久化，不代替浏览器中展开工作过程、观察思考流及统计栏的实测。

默认执行 14 个场景；`sandbox_files` 使用两个用户、两个容器作用域并发执行真实沙箱文件写入与读取，核对磁盘内容及作用域隔离，并检查沙箱数据目录未生成 SQLite 或会话 JSONL。

`idle_storage` 完成真实请求、关闭 WebSocket，等待所有准入轮次结束并静置 3 秒，随后无 HTTP 请求地观测。`--idle-seconds 600` 用于十分钟验收；省略该选项时仅观测 12 秒，不能算作十分钟通过。采样只读取服务容器主进程的 CPU 累计时间与 RSS，单核平均 CPU 超过 1% 即失败；同时对八张 canonical、admission、展示业务表比较 insert/update/delete 计数，新增业务写入即失败。隔离测试数据库启用 `pg_stat_statements`，审计使用独立角色，核验服务角色的历史 SELECT 增量为零，并检查统计重置、淘汰与活动历史查询。生产部署不要求开启该扩展。报告只保留计数、采样时长及 CPU/RSS，不保存 SQL 或参数；PostgreSQL 自身维护和服务其他后台活动单独归因，不能拿全库 WAL 或磁盘写入量替代。

每次使用新的 `--output`，报告为 `report.json`、`report.md`、`summary.csv`、逐次 `samples.jsonl`。原始合成数据保留在 `fixture/`，不入库。脚本完成后返回 0 表示通过，1 表示正确性失败、清理失败、基线不兼容或性能退化。

建立与比较基线：

```powershell
python scripts/experience-bench/measure-hive.py --server-binary target/x86/release/wunder-server --output temp_dir/hive-before --profile daily
python scripts/experience-bench/measure-hive.py --server-binary target/x86/release/wunder-server --output temp_dir/hive-after --profile daily --baseline temp_dir/hive-before/report.json
python scripts/experience-bench/measure-cli.py --binary target/release/wunder-cli.exe --output temp_dir/cli-before --profile daily
python scripts/experience-bench/measure-cli.py --binary target/release/wunder-cli.exe --output temp_dir/cli-after --profile daily --baseline temp_dir/cli-before/report.json
```

基线要求环境、场景、轮次、模型块数、调用方式和实际生成数量一致。蜂巢还核对 Docker 版本、镜像 ID 与容量。二进制 SHA256 独立记录，允许比较代码优化前后的构建。P50 或 P95 同时增加超过 20% 和 5 ms 时失败。单次 smoke 不能作稳定分位数基线；需单独记录机器负载、编译 features、缓存状态、数据库规模与发行环境。

慢消费者场景只有有限延迟，默认负载不保证填满内核管道或 WebSocket 队列，不能当作溢出回放已验证。API 测试不含浏览器绘制；CLI 测试不含 TUI 绘制。
