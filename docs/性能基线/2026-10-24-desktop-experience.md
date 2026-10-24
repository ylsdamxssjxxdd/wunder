# 蛹体验测试首次自动化验证

执行口径见 [蛹体验测试方案](../蛹体验测试方案.md)，结构化结果见 [JSON](2026-10-24-desktop-experience.json)。本记录用于验证测试链路，不是正式发布的性能门槛。

## 本次已验证

- 统一命令集成 100×100 大日志的七项读取场景，内容、分页、游标和权威日志哈希校验通过。
- Release 探针的 11 个体验场景全部通过：新环境启动、正常退出后重启恢复、提交、流式、慢消费、取消后继续、队列调序/撤下/插话执行、历史线程切换、后台执行保持、目录分页、短时连续运行。
- 额外一分钟连续运行完成 32 轮真实提交与切换，内容和持久化校验通过，资源采样未截断。
- 额外 96 块 / 1 ms 源端、每次增量消费后暂停 20 ms 的慢消费者通过；缓冲峰值 45 条，没有达到 128 条容量。
- 最终 Release 探针重新运行 11 个体验场景全部通过，新增的预览/权威正文分别校验和模型请求到达时间均已实际验证。
- 最后通过一条统一命令再次运行聊天打开七项及 11 个体验场景，完整通过（`final_unified`）。
- 最终探针的 1024 块 / 1 ms 慢消费压力通过：事件缓冲达到 128 条容量，逐字实时内容与权威正文完全一致，资源采样未截断。
- 再次运行完整 smoke 与前一次比较：正确性全部通过，基线兼容；聊天首次打开和上一页的 P50/P95 共 4 个指标超出阈值，总结果返回非零。测试时仍有并行编译，不能据此认定代码退化。
- 最终源码 `cargo check` 通过；统计、超时清理、单位、失败计数、基线兼容及资源趋势共 17 项 Python 测试通过。

## 测量限制

本次为开发机 smoke，每个普通场景只有一个独立进程样本，无法建立可靠 P95。并行编译污染耗时；应在安静环境按 daily 配置重新建立优化基线。

一分钟运行增加了真实历史，排除前 20% 后 RSS 起止增加约 1.97 MiB、私有内存约 2.09 MiB，句柄净增 3、线程净减 3。此结果只证明采样和趋势计算可运行，不能判断资源泄漏或证明长时稳定。

初版探针只识别旧的增量名称，后按正式 `thread_item_delta` / `source_event` 及嵌套数据封装修正，随后完整 smoke 通过。1024 块扩展实验的实时内容校验通过，但初版将有界聊天预览当全文比较，产生了错误失败。最终源码改为分别校验 2048 字节预览与权威正文，并补充模型请求到达时间；最终 Release 构建、全场景 smoke 和 1024 块压力均通过。构建期间存在外部终止及共享源码并行变动，耗时明显偏长，不计入场景时间。归档的 `final_smoke` / `slow_consumer_1024` 对应最终探针，早期报告不包含这两个新增检查点。

2 小时 soak、Win7 x86、Ubuntu 18.04 x64、工具阻塞取消、双线程同时输出和故障强杀恢复尚未执行。详见方案的覆盖表和扩充顺序。

## 复测

```powershell
python scripts/desktop-bench/measure-experience.py --profile smoke --output target/desktop-experience/smoke-next
python scripts/desktop-bench/measure-experience.py --profile daily --output target/desktop-experience/daily-next
python scripts/desktop-bench/measure-experience.py --cases slow_consumer --chunks 1024 --interval-ms 1 --output target/desktop-experience/backpressure-next
```

报告保留探针哈希、工作负载、环境和源码状态，不记录配置内容、实际用户数据或机器绝对路径。
