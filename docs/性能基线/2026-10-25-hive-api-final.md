# 蜂巢 API 外部体验测试（含空闲存储验收）

13/13 场景通过，本次隔离 Compose 容器、卷及生成凭证已清理。修复并发初始化后重跑，保留旧报告。

100 用户轮次 × 每轮 100 模型请求，另含 7,954 次工具调用；17,954 次模型动作、18,054 个展示条目、46,409 条 canonical 记录（含线程初始化元数据）。数据生成 90.852 秒，不计入聊天读取。

构建：Linux x86_64 release，wunder-server 默认 postgres-storage，Rust 1.92.0。SHA256：`4e956019c80a74a20d63dd4e77a3d8dd8d52719d3eece572139e6a51337a131f`。

共享开发环境同期有编译负载，未清理操作系统缓存。smoke 每场景仅一次；daily 的 P95 是 10 个样本的最大值。这是对应构建的后端 API 验收，不含浏览器绘制，不代替目标环境 release 验收；不对旧版报告声称提速比例。

空闲观测 12 秒：服务 CPU 10 ms，单核平均 0.083%，RSS 峰值 52.00 MiB，八张线程业务表写入增量全部为 0。该窗口不代表长时间资源趋势。

# hive-api-experience

正确性：True；总结果：True。

| 场景 | 成功/运行 | 指标 | P50 ms | P95 ms | 最大 ms |
| --- | ---: | --- | ---: | ---: | ---: |
| login | 1/1 | login_ms | 40.160 | 40.160 | 40.160 |
| session_catalog | 1/1 | catalog_ms | 21.753 | 21.753 | 21.753 |
| session_catalog | 1/1 | create_ms | 19.833 | 19.833 | 19.833 |
| session_catalog | 1/1 | empty_bootstrap_ms | 25.557 | 25.557 | 25.557 |
| stream | 1/1 | durable_read_ms | 18.804 | 18.804 | 18.804 |
| stream | 1/1 | provider_stream_ms | 58.617 | 58.617 | 58.617 |
| stream | 1/1 | provider_to_first_ms | 8.247 | 8.247 | 8.247 |
| stream | 1/1 | send_to_first_text_ms | 502.301 | 502.301 | 502.301 |
| stream | 1/1 | send_to_provider_ms | 492.107 | 492.107 | 492.107 |
| stream | 1/1 | send_to_terminal_ms | 612.272 | 612.272 | 612.272 |
| slow_consumer | 1/1 | durable_read_ms | 22.234 | 22.234 | 22.234 |
| slow_consumer | 1/1 | provider_stream_ms | 58.442 | 58.442 | 58.442 |
| slow_consumer | 1/1 | provider_to_first_ms | 8.250 | 8.250 | 8.250 |
| slow_consumer | 1/1 | send_to_first_text_ms | 379.316 | 379.316 | 379.316 |
| slow_consumer | 1/1 | send_to_provider_ms | 366.510 | 366.510 | 366.510 |
| slow_consumer | 1/1 | send_to_terminal_ms | 607.762 | 607.762 | 607.762 |
| cancel_resume | 1/1 | cancel_ack_ms | 37.135 | 37.135 | 37.135 |
| cancel_resume | 1/1 | cancel_to_settled_ms | 54.852 | 54.852 | 54.852 |
| cancel_resume | 1/1 | durable_read_ms | 20.360 | 20.360 | 20.360 |
| cancel_resume | 1/1 | provider_stream_ms | 58.427 | 58.427 | 58.427 |
| cancel_resume | 1/1 | provider_to_first_ms | 8.225 | 8.225 | 8.225 |
| cancel_resume | 1/1 | send_to_first_text_ms | 280.294 | 280.294 | 280.294 |
| cancel_resume | 1/1 | send_to_provider_ms | 250.735 | 250.735 | 250.735 |
| cancel_resume | 1/1 | send_to_terminal_ms | 367.924 | 367.924 | 367.924 |
| queue | 1/1 | enqueue_ms | 56.257 | 61.163 | 61.163 |
| queue | 1/1 | queue_read_ms | 10.771 | 10.771 | 10.771 |
| queue | 1/1 | queued_cancel_ms | 45.330 | 45.330 | 45.330 |
| queue | 1/1 | reorder_ms | 15.567 | 15.567 | 15.567 |
| parallel_threads | 1/1 | batch_ms | 3537.545 | 3537.545 | 3537.545 |
| parallel_threads | 1/1 | durable_read_ms | 22.472 | 22.593 | 22.593 |
| parallel_threads | 1/1 | provider_stream_ms | 59.104 | 59.835 | 59.835 |
| parallel_threads | 1/1 | provider_to_first_ms | 8.252 | 8.368 | 8.368 |
| parallel_threads | 1/1 | send_to_first_text_ms | 343.704 | 361.629 | 361.629 |
| parallel_threads | 1/1 | send_to_provider_ms | 335.961 | 353.177 | 353.177 |
| parallel_threads | 1/1 | send_to_terminal_ms | 450.300 | 466.914 | 466.914 |
| isolation | 1/1 | anonymous_rejection_ms | 3.450 | 3.450 | 3.450 |
| isolation | 1/1 | cross_user_rejection_ms | 6.949 | 8.870 | 8.870 |
| workspace | 1/1 | file_read_ms | 32.623 | 32.623 | 32.623 |
| workspace | 1/1 | first_directory_page_ms | 113.380 | 113.380 | 113.380 |
| workspace | 1/1 | older_directory_page_ms | 75.628 | 75.628 | 75.628 |
| large_chat | 1/1 | body_read_ms | 17.035 | 17.035 | 17.035 |
| large_chat | 1/1 | chat_bootstrap_ms | 30.802 | 30.802 | 30.802 |
| large_chat | 1/1 | newer_chat_page_ms | 26.059 | 26.059 | 26.059 |
| large_chat | 1/1 | older_chat_page_ms | 24.722 | 24.722 | 24.722 |
| restart_recovery | 1/1 | durable_read_ms | 30.794 | 30.794 | 30.794 |
| restart_recovery | 1/1 | provider_stream_ms | 59.616 | 59.616 | 59.616 |
| restart_recovery | 1/1 | provider_to_first_ms | 8.343 | 8.343 | 8.343 |
| restart_recovery | 1/1 | restart_to_ready_ms | 1356.957 | 1356.957 | 1356.957 |
| restart_recovery | 1/1 | restored_chat_read_ms | 70.730 | 70.730 | 70.730 |
| restart_recovery | 1/1 | send_to_first_text_ms | 402.487 | 402.487 | 402.487 |
| restart_recovery | 1/1 | send_to_provider_ms | 389.691 | 389.691 | 389.691 |
| restart_recovery | 1/1 | send_to_terminal_ms | 543.667 | 543.667 | 543.667 |
| storage_audit | 1/1 | storage_audit_ms | 514.546 | 514.546 | 514.546 |
| idle_storage | 1/1 | idle_cpu_ms | 10.000 | 10.000 | 10.000 |
| idle_storage | 1/1 | idle_observation_ms | 11999.978 | 11999.978 | 11999.978 |

计时为外部单调时钟；数据准备单列，不含浏览器/TUI 绘制。smoke 不能建立可靠 P95。
环境、负载与调用方式一致才比较；失败、超时和降级页不能当成功样本。完整证据见配套 JSON。
