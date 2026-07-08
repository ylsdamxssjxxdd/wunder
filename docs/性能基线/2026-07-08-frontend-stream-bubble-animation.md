# 2026-07-08 用户侧流式气泡与动画卡顿

## 目标

降低本地模型高频流式输出时，聊天气泡正文更新对头像动画、浮动形象动画和工作区交互的主线程干扰。

## 环境

- 入口：用户侧前端聊天页
- 数据来源：浏览器聊天 debug 导出
- 状态：工作区存在其他未提交改动，本记录只覆盖本次前端流式渲染链路调整

## 基线结果

- debug 导出共 294 条事件，其中 `chat.stream.perf/send-content-event` 197 条，说明前端持续收到流式事件。
- `chat.stream.perf/content-clock-slow-flush` 出现 5 次，最大延迟 642ms。
- `chat.stream.perf/message-body-stream-render` 中，多次短正文进入 `plainStreaming:false`，流式阶段触发 Markdown/HTML 渲染路径。
- 非轻量流式正文渲染后会触发消息视口测量和贴底滚动，气泡 DOM 排版与动画共享主线程。

## 调整

- 流式正文阶段统一使用轻量纯文本预览，结束后再进入完整 Markdown 渲染。
- 纯文本可见内容写入按约 32ms 合并，内容投影时钟按约 24ms 合并。
- 轻量流式正文的贴底滚动降频到约 160ms，终态 Markdown 仍立即测量和贴底。
- 浮动形象雪碧帧从 Vue/JS 定时器切到 CSS `steps()` 动画，避免每帧进入响应式更新。
- 复测发现头像与浮动形象会停留在第一帧，根因是动态内联 `animation` 引用了 `scoped` 样式中的 keyframes；已将 `companion-sprite-step` 保持为非 scoped 全局 keyframes，让 CSS 帧动画真正推进。

## 优化后验证

- `npm run test:messenger-renderable-source`：通过
- `npm run test:message-viewport-runtime`：通过
- `npm run test:message-render`：通过
- `npm run typecheck`：通过

## 对比

| 指标 | 基线 | 调整后 |
| --- | --- | --- |
| 内容时钟慢刷新 | 5 次，最大 642ms | 待浏览器复测 |
| 流式正文渲染路径 | 短正文可进入 Markdown/HTML 路径 | 流式阶段统一轻量文本预览 |
| 贴底滚动 | 可随每次正文 rendered 触发 | 轻量流式约 160ms 合并 |
| 浮动形象帧动画 | JS 定时器驱动 Vue 状态 | CSS steps 动画 |

## 结论

代码层已移除本次 debug 暴露的主要主线程放大点；实际体感和 `content-clock-slow-flush` 是否归零仍需同一聊天场景复测。若复测仍有卡顿，下一步应采集浏览器 Performance trace，重点看 layout、style recalculation 和本地模型进程资源争用。
