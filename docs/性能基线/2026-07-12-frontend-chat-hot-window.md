# 聊天长会话热窗口验收记录

## 目标

验证聊天页的长会话热集收敛不会破坏历史补水、投影物化、滚动位置或类型安全。

本次改动将首开详情页、历史补水页、内存消息窗口和 projection 物化缓存收敛到有界范围，并避免为无对应数据的助手消息创建工作流、子智能体、引用和思考组件。

## 环境与入口

- 日期：2026-07-12
- 入口：`frontend` 的 Node 回归、Vue 类型检查与 Playwright 重历史 harness。
- 数据规模：Playwright harness 生成 400 条大 Markdown 消息，并执行顶部/底部滚动和追加消息。
- 注意：当前 harness 使用 Messenger 样式与 viewport runtime，但不是完整 `MessengerView` 真实后端会话。它用于防止结构性回归，不构成真实设备上的前后性能百分比对比。

## 配置变化

| 项目 | 变更前 | 变更后 |
| --- | ---: | ---: |
| Web 首开历史详情上限 | 500 条 | 80 条 |
| Web 常驻消息窗口 | 400 条 | 120 条 |
| Web 窗口最大值 | 2000 条 | 320 条 |
| 历史补水页 | 80 条 | 40 条 |
| Desktop 常驻消息窗口 | 96 条 | 64 条 |
| Desktop 窗口最大值 | 640 条 | 192 条 |
| 投影物化缓存单会话上限 | 5000 条 | 320 条 |
| Markdown HTML 缓存 | 240 条 | 240 条且总量不超过 12 MiB |
| 超长历史正文初始 DOM | 完整正文 | 24,000 字符分段，用户展开后加载完整正文 |
| 单个工作流展开详情 | 无上限 | 最近 3 个，未展开条目不挂载详情 DOM |

早于热窗口的历史仍通过既有稳定 `before_id` 游标按需加载；本次没有修改消息顺序、事件投影或 durable history。

## 验证命令与结果

| 命令 | 结果 |
| --- | --- |
| `npm run test:chat-history-backfill` | 通过，验证重复页跳过与时间顺序正确。 |
| `npm run test:chat-runtime-render-adapter` | 通过，29 项，验证稳定 key、流式物化与投影渲染。 |
| `npm run test:message-viewport-runtime` | 通过，7 项，验证滚动不执行同步行测量、补历史后位置稳定。 |
| `npm run typecheck:vue` | 通过。 |
| `npm run test:e2e:messenger-heavy-history` | 通过，2 项；400 条大 Markdown 消息的滚动与追加场景通过。 |
| `cargo check -j 8 -p wunder-core -p wunder-runtime --features sqlite-storage` | 通过，验证摘要详情 API 与 SQLite 存储实现可编译。 |

Playwright 期间 Vite 对未启动的 `wunder-server` 记录了 `/wunder/i18n` 代理 DNS 错误，但测试 harness 未依赖该接口，两个用例均通过。

`cargo test` 的摘要协议定向用例在本机 120 秒命令窗口内仍处于 runtime 测试目标编译阶段而超时，未得到测试断言结果；不得将其视为通过，后续应在预热的 Rust 测试环境继续运行。

补充验证：在依赖缓存预热后，`cargo test -j 8 -p wunder-runtime transcript_summary_preserves_identity_and_marks_large_fields --lib --features sqlite-storage` 已通过（1 passed）。同时完成 `npm run test:messenger-renderable-source`、`npm run test:workspace-refresh`、`npm run test:message-viewport-runtime`、`npm run typecheck:vue` 与重历史 Playwright 场景回归。工作区资源下载现在以页面生命周期绑定的 `AbortController` 管理，缓存清理或页面卸载会取消未完成请求；摘要详情补水会清除正文、推理、工作流与子智能体的全部截断标记。

后续硬边界补齐：虚拟消息行高缓存采用 384 条 LRU 上限，超出时淘汰最久未测量的行高；工作区 blob URL 缓存采用 48 条上限，只回收未被当前图片元素或资源预览引用的最旧 URL，避免回收可见资源导致预览闪烁。已运行 `npm run test:message-viewport-runtime`（8 项通过）、`npm run test:messenger-renderable-source`（18 项通过）、`npm run test:chat-history-backfill`、`npm run test:chat-runtime-render-adapter`（29 项通过）、`npm run test:message-virtual-window`、`npm run test:message-render` 和重历史 Playwright 场景（2 项通过）。

## 真实 MessengerView 页面回归

新增 `npm run test:e2e:messenger-view-performance`，直接挂载生产 `MessengerView`、生产控制器和生产消息组件树，不使用简化的全量 `v-for` 页面。固定场景包含 320 条长会话历史、40 条向上补页、快速往返滚动、连续正文更新、工具详情展开尝试、切换到第二会话后返回。场景采集首个可交互时间、最大帧间隔、DOM 节点数、实际挂载消息数、展开工具详情数、请求数和浏览器可提供时的 JS heap。

本机 Chromium 回归通过以下保护门槛：首个可交互时间小于 8 秒、最大帧间隔小于 250 毫秒、实际挂载消息少于 40 条、展开详情不超过 3 条、DOM 节点少于 5000、请求数少于 80。门槛用于防止结构性回退，不代表 Win7 或其他真实用户设备的绝对性能承诺；仍不宣称没有同环境优化前采样支撑的百分比提升。

## 结论

本轮完成有界热集的结构性收敛并通过功能回归。由于没有同一真实设备、同一完整 `MessengerView` 会话的优化前采样，本报告不宣称具体性能提升百分比。

后续必须补齐真实页面基线：使用固定长会话 fixture，采集首个消息 shell 可见时间、可输入时间、滚动最大帧间隔、DOM 节点、JS heap 和网络页数；同环境重复采样后再判定性能提升或回退。
