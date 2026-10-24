# 蛹聊天打开后端基准

统一场景、复测频率和优化流程见 [蛹体验测试方案](../../docs/蛹体验测试方案.md)。本基准作为该方案的 UX-01，被 `measure-experience.py` 直接复用。

这是开发用的外部测试，直接链接 `wunder-desktop` 和 `wunder-runtime`，调用聊天页面实际使用的 `NativeDesktop::open_chat_snapshot()`。不构建或启动最终蛹程序，不依赖 Slint、桌面窗口、浏览器、HTTP bridge、模型或补充包。测试工作进程位于 `crates/wunder-desktop/examples/chat_open_probe.rs`，不会编入蛹的产品入口。

需要 Rust 工具链、Python 3.8+ 和 `psutil`。Windows 7 使用与该系统兼容的 Python/psutil；跨平台脚本本身不依赖新 Windows API。当前机器上的通过不能替代 Win7 或 Ubuntu 18.04 真机测试。

```powershell
python -m pip install psutil
python scripts/desktop-bench/measure-chat-open.py --output target/desktop-chat-open/baseline
```

一条命令自动构建 Release 测试工作进程、生成固定 100×100 数据、执行所有场景各 1 次预热和 10 次测量，输出 `report.json`、`report.md`、`summary.csv` 和逐次追加的 `samples.jsonl`。输出目录必须是新目录；数据默认保存在 `target/desktop-chat-open/fixture`，后续运行复用同一份数据。

## 负载与正确性

“100×100”严格表示 100 个用户轮次，每轮 100 个模型动作；模型请求和工具执行各计一个动作。默认每轮 67 次模型请求、33 次工具执行，共 10,000 个动作，加上 100 条用户消息，共 10,100 个可见条目，最后一个模型请求不带工具。数据清单分别记录两类动作数，避免把 10,000 次模型请求额外附加工具后仍称作 100×100。

默认固定种子 42，ASCII 合成回复主体 1,024 字节、思考 512 字节、工具结果主体 4,096 字节，另有固定的元数据和正文标记。种子生成的文本可复现；存储身份和接入时间由正式后端分配，因此重新生成后的物理摘要会变化。持续比较应复用同一个 fixture，而不是每次重新生成。

夹具走正式接入、规范日志写入、持久化策略和历史投影，不插入手写 SQL 行，不放宽 durability，也不调用任何真实工具。创建时冻结合成系统提示词。准备在独立 worker 中完成后退出，计时 worker 不继承生成进程的内存缓存。

数据目录通过原子创建和所有权标记隔离。`WUNDER_HOME`、存储根和工作目录指向夹具；既有目录不覆盖，脚本不删除目录。每次成功返回必须通过正文标记、轮次连续性、最新轮次、历史范围和重放游标检查。全部测量前后比较权威 rollout 文件 SHA-256，读操作改变正文则失败。元数据、租约和 SQLite WAL 正常变化不纳入不可变摘要。

## 测量边界

| case | 操作 | 解释 |
| --- | --- | --- |
| `first_open` | 新进程中首次调用 `open_chat_snapshot()` | 主指标，含线程打开租约、会话统计、首批聊天数据及同一读事务的重放游标、native 结果构建 |
| `cached_reopen` | 先打开一次，再计时第二次调用 | 同一进程已有租约和 30 秒内快照缓存的返回成本，不能替代首次打开 |
| `native_headers` | 直接调用 `load_chat_headers()` | 绕过快照缓存，读取与转换首批 native 历史 |
| `storage_page` | `thread_view_page(..., "chat", ..., 12)` | 共享存储层首批聊天读模型处理成本 |
| `session_info` | `get_session_info()` | 会话信息、统计与打开租约路径 |
| `change_cursor` | `latest_thread_change_seq_by_session()` | 独立 durable 重放游标读取；初次打开已直接复用聊天页游标，不再单独调用 |
| `older_page` | 初次打开取得游标后计时下一页 | 聊天历史向上翻页；准备首批数据的时间不计入 |

每个场景的每个样本在新进程执行。计时使用 Rust 单调时钟，在接口返回时立即停止，再做断言与 JSON 输出。运行时启动另列 `startup_ms`，不计入主指标。每个场景都保留首次连接/进程缓存的真实成本；分项在不同进程独立执行，不允许相减或相加来声称某内部函数的独占耗时。

首批聊天页当前为 12 个用户轮次。该基准测量大线程背景下准备这批展示数据的耗时，**不是全文解析/导出耗时，也不是用户看见完整首屏的时间**。前端的 Markdown 分块、模型应用、布局、绘制和事件观察器接管仍不在测量内。正式接口改变分页策略后需重新检查正确性和比较口径。

进程重启不清除操作系统文件缓存，`first_open` 不称为磁盘冷读。共享目录重复使用时可能出现后端启动维护；启动时间单列，后台任务的竞争仍会反映在测量中。固定机器、系统、磁盘、工具链及构建参数运行，避免在编译或其他高负载任务同时运行时采集正式基线。

聊天摘要由正式日志投影同步维护，只存消息身份、计数和工作区变更水位，打开时按索引读取需要的消息预览。分支按不可变截止点查询摘要检查点，源线程后续续写不会改变已有分支。旧夹具首次读取会按访问到的 rollout 补建摘要，每批最多 128 条或约 2 MiB，可中断后继续，不改写权威日志。升级耗时需要单独测量时使用 `--cases first_open --warmups 0 --runs 1`；常规带预热的基线比较衡量补建后的稳定性能。摘要及检查点占用额外 SQLite 空间，不能只比较打开耗时而忽略写入和空间成本。

## 持续比较

```powershell
python scripts/desktop-bench/measure-chat-open.py --runs 30 --output target/desktop-chat-open/next --baseline target/desktop-chat-open/baseline/report.json
```

报告列出 P50、P95、最大值、成功/失败次数、CPU 时间、内存增量及读写计数。P95 使用 nearest-rank；失败和超时不会填成 0 或当成功样本剔除，任一失败令脚本以非零码退出。10 次适合开发观察，正式归档建议至少 30 次。

基线比较必须满足相同数据摘要、环境、构建形态和测量模式。每个场景 P50/P95 同时超过默认 `20%` 和 `5 ms` 时判为退化，可用 `--regression-percent`、`--regression-ms` 调整；条件不一致直接拒绝比较。产物 SHA-256、Git 版本与工作区是否修改另行记录，允许优化后代码与二进制变化。不要把不同机器或磁盘的结果直接比较；报告环境清单不包含主机名、用户名和绝对目录。

重复测量已构建的独立 worker 可以指定 `--probe`，这里传的是测试程序，而非蛹 EXE。更换后端代码后默认不传该参数，脚本会重新构建，避免误测旧程序。

较小的连通性检查：

```powershell
python scripts/desktop-bench/measure-chat-open.py --user-rounds 16 --actions 6 --fixture target/desktop-chat-open/smoke-fixture --runs 1 --warmups 0 --output target/desktop-chat-open/smoke
```

可用 `--cases first_open,native_headers,storage_page` 只测指定场景。`--prepare-only` 仅生成夹具；后续测量必须使用相同负载参数。

## 定位瓶颈

先查看 `first_open` 与各分项分布，再录制耗时最高的分项调用栈。已有资源计数由独立 Python 进程通过 psutil 采集，默认约 10 ms 一次，窗口是 worker 握手到结果返回。内存峰值是采样峰值；亚毫秒调用可能捕捉不到临时分配，CPU 与 I/O 计数也受系统粒度影响。Windows I/O transfer bytes 是进程逻辑读取统计，Linux read_bytes 更接近实际存储读取，不能跨系统直接对齐。

录制诊断与正式计时分开。循环参数只用于让采样 profiler 收集足够栈；首个调用后连接缓存会变热，循环分布不能替代首次打开。`first_open` 强制每进程只执行一次。

Windows 的 WPR GeneralProfile 需要系统支持及足够权限。录制器启动失败（包括已有录制会话）会记录失败，不接管或取消他人的录制。脚本在启动握手前开启系统录制，完成后保存 ETL，用 WPA 按 worker PID/时间窗口分析 CPU Sample、CPU Usage Precise 等待、文件 I/O 和缺页。ETL 是外部本机诊断产物，可能包含系统路径；不自动写入仓库或汇总文档。

```powershell
python scripts/desktop-bench/measure-chat-open.py --symbols --cases native_headers --iterations 100 --runs 1 --warmups 0 --profile wpr --output target/desktop-chat-open/profile
```

Linux：

```bash
python3 scripts/desktop-bench/measure-chat-open.py --symbols --cases native_headers --iterations 100 --runs 1 --warmups 0 --profile perf --output target/desktop-chat-open/profile-linux
perf report -i target/desktop-chat-open/profile-linux/native_headers-0.perf.data
```

`--symbols` 只为测试构建保留调试信息，不更改产品发布配置。诊断报告标注 profiler 和循环次数，禁止直接与普通基线比较。WPR/perf 依赖系统工具与权限；无这些工具时普通计时、资源计数与分项定位仍可自动完成。
