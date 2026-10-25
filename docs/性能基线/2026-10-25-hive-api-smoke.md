# 蜂巢 API 外部体验测试

Linux release 舰体通过独立 Docker Compose 启动，12/12 场景通过；生成凭证、容器和卷已清理。

大线程：100 用户轮次 × 每轮 100 模型请求，另含 7,954 次工具调用，共 17,954 次模型动作、18,054 个展示条目、46,408 条 canonical 记录；生成 83.704 秒，单独计时。聊天首屏 12 轮约 42 KiB。全栈 canonical 逻辑数据约 72.1 MiB；物理数据库容量见 JSON 中 postgres_storage，包含索引、页面和数据库开销。

构建：Linux x86_64 release，wunder-server 默认 postgres-storage，Rust 1.92.0；二进制 SHA256 见配套 JSON。旧 smoke 的负载契约和运行镜像不同，不做提速比例对比。共享开发环境同期存在编译负载，以下每场景仅一次；用于链路验收，不代表稳定 P95 或发行平台验收。

# hive-api-experience

正确性：True；总结果：True。

| 场景 | 成功/运行 | 指标 | P50 ms | P95 ms | 最大 ms |
| --- | ---: | --- | ---: | ---: | ---: |
| login | 1/1 | login_ms | 63.395 | 63.395 | 63.395 |
| session_catalog | 1/1 | catalog_ms | 37.648 | 37.648 | 37.648 |
| session_catalog | 1/1 | create_ms | 31.385 | 31.385 | 31.385 |
| session_catalog | 1/1 | empty_bootstrap_ms | 36.754 | 36.754 | 36.754 |
| stream | 1/1 | durable_read_ms | 32.357 | 32.357 | 32.357 |
| stream | 1/1 | provider_stream_ms | 58.986 | 58.986 | 58.986 |
| stream | 1/1 | provider_to_first_ms | 8.433 | 8.433 | 8.433 |
| stream | 1/1 | send_to_first_text_ms | 741.881 | 741.881 | 741.881 |
| stream | 1/1 | send_to_provider_ms | 730.865 | 730.865 | 730.865 |
| stream | 1/1 | send_to_terminal_ms | 897.708 | 897.708 | 897.708 |
| slow_consumer | 1/1 | durable_read_ms | 84.393 | 84.393 | 84.393 |
| slow_consumer | 1/1 | provider_stream_ms | 58.493 | 58.493 | 58.493 |
| slow_consumer | 1/1 | provider_to_first_ms | 8.302 | 8.302 | 8.302 |
| slow_consumer | 1/1 | send_to_first_text_ms | 602.556 | 602.556 | 602.556 |
| slow_consumer | 1/1 | send_to_provider_ms | 592.908 | 592.908 | 592.908 |
| slow_consumer | 1/1 | send_to_terminal_ms | 832.207 | 832.207 | 832.207 |
| cancel_resume | 1/1 | cancel_ack_ms | 60.664 | 60.664 | 60.664 |
| cancel_resume | 1/1 | cancel_to_settled_ms | 87.052 | 87.052 | 87.052 |
| cancel_resume | 1/1 | durable_read_ms | 27.053 | 27.053 | 27.053 |
| cancel_resume | 1/1 | provider_stream_ms | 58.546 | 58.546 | 58.546 |
| cancel_resume | 1/1 | provider_to_first_ms | 8.332 | 8.332 | 8.332 |
| cancel_resume | 1/1 | send_to_first_text_ms | 401.593 | 401.593 | 401.593 |
| cancel_resume | 1/1 | send_to_provider_ms | 392.392 | 392.392 | 392.392 |
| cancel_resume | 1/1 | send_to_terminal_ms | 543.835 | 543.835 | 543.835 |
| queue | 1/1 | enqueue_ms | 84.195 | 102.134 | 102.134 |
| queue | 1/1 | queue_read_ms | 14.839 | 14.839 | 14.839 |
| queue | 1/1 | queued_cancel_ms | 107.372 | 107.372 | 107.372 |
| queue | 1/1 | reorder_ms | 23.624 | 23.624 | 23.624 |
| parallel_threads | 1/1 | batch_ms | 3762.921 | 3762.921 | 3762.921 |
| parallel_threads | 1/1 | durable_read_ms | 25.647 | 25.867 | 25.867 |
| parallel_threads | 1/1 | provider_stream_ms | 58.146 | 58.270 | 58.270 |
| parallel_threads | 1/1 | provider_to_first_ms | 8.291 | 8.294 | 8.294 |
| parallel_threads | 1/1 | send_to_first_text_ms | 531.763 | 536.234 | 536.234 |
| parallel_threads | 1/1 | send_to_provider_ms | 522.938 | 527.946 | 527.946 |
| parallel_threads | 1/1 | send_to_terminal_ms | 667.905 | 668.346 | 668.346 |
| isolation | 1/1 | anonymous_rejection_ms | 3.959 | 3.959 | 3.959 |
| isolation | 1/1 | cross_user_rejection_ms | 10.314 | 11.224 | 11.224 |
| workspace | 1/1 | file_read_ms | 73.130 | 73.130 | 73.130 |
| workspace | 1/1 | first_directory_page_ms | 180.515 | 180.515 | 180.515 |
| workspace | 1/1 | older_directory_page_ms | 191.187 | 191.187 | 191.187 |
| large_chat | 1/1 | body_read_ms | 13.593 | 13.593 | 13.593 |
| large_chat | 1/1 | chat_bootstrap_ms | 30.618 | 30.618 | 30.618 |
| large_chat | 1/1 | newer_chat_page_ms | 25.754 | 25.754 | 25.754 |
| large_chat | 1/1 | older_chat_page_ms | 25.209 | 25.209 | 25.209 |
| restart_recovery | 1/1 | durable_read_ms | 24.911 | 24.911 | 24.911 |
| restart_recovery | 1/1 | provider_stream_ms | 58.102 | 58.102 | 58.102 |
| restart_recovery | 1/1 | provider_to_first_ms | 8.334 | 8.334 | 8.334 |
| restart_recovery | 1/1 | restart_to_ready_ms | 1386.925 | 1386.925 | 1386.925 |
| restart_recovery | 1/1 | restored_chat_read_ms | 62.342 | 62.342 | 62.342 |
| restart_recovery | 1/1 | send_to_first_text_ms | 538.589 | 538.589 | 538.589 |
| restart_recovery | 1/1 | send_to_provider_ms | 527.122 | 527.122 | 527.122 |
| restart_recovery | 1/1 | send_to_terminal_ms | 687.819 | 687.819 | 687.819 |
| storage_audit | 1/1 | storage_audit_ms | 541.277 | 541.277 | 541.277 |

计时为外部单调时钟；数据准备单列，不含浏览器/TUI 绘制。smoke 不能建立可靠 P95。
环境、负载与调用方式一致才比较；失败、超时和降级页不能当成功样本。完整证据见 report.json。
