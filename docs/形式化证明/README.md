# 形式化证明与状态验证

本目录是 wunder 形式化模型、状态验证、反例和证明证据的唯一归档入口。蛹是本地 AI 工作台，相关证明优先保障消息、执行、停止与本地数据安全；共享引擎及其他入口的证据按各主题声明适用范围，不相互扩大结论。

目录名称不表示所有内容都是无限状态定理证明。已有资料包含有限模型检查、真实入口回归和部分未完成项；每份结论必须连同假设、边界和失败清单一起阅读。

## 总清单与汇总

- [形式化验证必做清单](./形式化验证必做清单.md)：风险排序、待办范围、局部证据与打勾门槛。
- [问询、上下文压缩、子智能体回归汇总](./fv-11-13-14-regression.md)：三个主题的真实代码回归及验证边界。

## 主题索引

| 主题 | 模型与证据入口 | 范围 |
| --- | --- | --- |
| 线程执行持有权 | [fv-01-thread-ownership](./fv-01-thread-ownership/README.md) | 租约接管、迟到释放与心跳隔离 |
| 消息受理 | [fv-02-message-admission](./fv-02-message-admission/README.md) | 幂等、崩溃孤儿结算与派发 |
| canonical 提交恢复 | [fv-03-canonical-commit-recovery](./fv-03-canonical-commit-recovery/README.md) | 文件先行与无头历史防御 |
| 停止与结清 | [fv-04-stop-cancel-settlement](./fv-04-stop-cancel-settlement/README.md) | 取消闸、终态守卫及迟到停止 |
| 模型与工具故障恢复 | [failure-recovery](./failure-recovery/README.md) | HTTP 分类、有限退避、截断流、取消结清与超时副作用建议 |
| 问询消费 | [fv-11-inquiry-consumption](./fv-11-inquiry-consumption/README.md) | 单次消费、回答受理与停止竞争 |
| 上下文检查点 | [fv-13-context-checkpoint](./fv-13-context-checkpoint/README.md) | 先持久化后应用、结清围栏 |
| 子智能体 | [fv-14-child-runs](./fv-14-child-runs/README.md) | 祖先取消、派生与迟到回收 |
| 目标控制 | [goal-control](./goal-control/README.md) | 创建、编辑、暂停与本地投影 |
| 目标轮次驱动 | [goal-round-driver](./goal-round-driver/README.md) | 会话栅栏释放后的继续驱动与丢失唤醒 |
| 多本地入口共存 | [local-entry-coexistence](./local-entry-coexistence/README.md) | 蛹与多个舵机共享数据根、线程隔离 |
| 定时任务 | [cron](./cron/README.md) | 领取、编辑、停用、删除与历史原子性 |
| 服务长线程 | [service-history-20261027](./service-history-20261027/README.md) | 诊断资源边界、元信息与投影计数 |
| 线程历史窗口外回读 | [thread-history-replay](./thread-history-replay/README.md) | 活动窗口裁剪、稀疏锚点、轮次起点、扫描预算与分页完整性 |
| 数据库 I/O 隔离 | [postgres-io](./postgres-io/README.md) | 业务运行时阻塞与独立网络驱动模型 |
| 蛹与蜂巢聊天投影 | [聊天状态一致性验证](./聊天状态一致性验证/README.md) | 完成头、过程刷新、实时条目与思考身份隔离 |
| 蛹聊天流式跟随 | [stream-follow-20261030](./stream-follow-20261030/README.md) | 外层时间线与工作过程列表的尾部跟随、条目静默变高反例 |
| 蛹与舵机投影 | [蛹与舵机状态一致性验证](./蛹与舵机状态一致性验证/README.md) | 终态、过程分页与线程状态 |
| 舵机 stream 轮次边界 | [tui-stream-epoch-20261027](./tui-stream-epoch-20261027/README.md) | 传输事件编号去重与新 stream 重置 |
| 舵机中断报错 | [tui-terminal-20261028](./tui-terminal-20261028/README.md) | 错误、取消终态、关闭与下一轮重置 |

## 后续存放规则

1. 新资料只写入 `docs/形式化证明/<主题>/`。已有主题继续维护原目录；独立时间快照才增加日期后缀，不再嵌套新的“技术说明书/验证记录”或在设计文档中复制模型。
2. 每个主题保留 `README.md`，写明契约、不变量、代码检查点、修复前后结果、真实回归、已知失败、未覆盖范围和重跑命令；在此索引登记，按需更新必做清单。
3. 模型与配置只有一份权威源。新主题推荐把 `.tla`、`.cfg`、`.map.mjs` 放在 `models/`；已有主题可保留原布局，避免无意义地改动复验路径。
4. 复验入口留在 `scripts/check_*_formal.py`，读取本目录的模型并复制到仓库外 scratch 运行；实际 Rust、前端和 Python 回归仍放原测试目录，不混入文档。
5. 可提交模型、配置、映射、README、摘要、编号反例 Mermaid 和必要 PNG。经脱敏且有证据用途的 TLC 文本输出可以保留，必须区分审查归档与临时运行日志。
6. 临时 `report/`、`states/`、缓存、原始交互日志和带机器路径或凭据的截图不得入库；完整重跑产物放仓库外。归档摘要的来源路径使用仓库相对路径或明确的 scratch 标识。
7. 迁移只调整路径，不更改既有证明规则、状态计数或通过范围。技术说明书、设计文档和性能基线以链接引用本目录，不重复保存证明材料；实际性能测量仍保留在 `docs/性能基线/`。

## 复验入口

准备已核验的 Java、TLC 与 statechart 技能，必要时设置 `JAVA` 和 `TLA2TOOLS`。脚本会拒绝将 `--out` 写入仓库；以下参数均为占位符。

```powershell
python scripts/check_cron_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/cron
python scripts/check_service_history_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/service-history
python scripts/check_thread_history_replay_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/thread-history-replay
python scripts/check_postgres_io_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/postgres-io
python scripts/check_work_process_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/work-process
```

其他主题按各自 README 的命令在仓库外复验；迁移和路径检查本身不构成重新运行全部 TLC 证明。

目录、索引、摘要可读性、预览导入及三个复验脚本的模型来源，可用轻量回归检查；其中 checker 被替身隔离，不会运行 Java/TLC：

```powershell
python -m unittest discover -s tests -p test_formal_proof_layout.py -v
```
