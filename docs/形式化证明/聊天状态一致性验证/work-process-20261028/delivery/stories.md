## 1. 工具实时条目<br/>执行中<br/>未展示条目，转圈

Rule broken: An admitted running tool is visible before its result; mutators: crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:611.
Why: tool.
In the model, one step leads into this bug: "工具开始即推送条目".

0. Start: 工具实时条目<br/>尚未启动<br/>未展示条目，转圈
1. 工具开始即推送条目. Now: 工具实时条目<br/>执行中<br/>未展示条目，转圈

With the fix, step 1 (工具开始即推送条目) takes you to: 工具实时条目<br/>执行中<br/>条目已展示，转圈.

## 2. 连续结构事件<br/>已收到3次事件<br/>首次期限3，当前期限4<br/>等待刷新

Rule broken: Later structural events cannot postpone an armed refresh; mutator: frontend/src/components/chat/MessageTimelineTurnProcess.vue:533.
Why: refresh.
In the model, one step leads into this bug: "新事件不推迟首次刷新期限".

0. Start: 连续结构事件<br/>已收到1次事件<br/>首次期限0，当前期限0<br/>等待刷新
1. 新事件不推迟首次刷新期限. Now: 连续结构事件<br/>已收到2次事件<br/>首次期限3，当前期限3<br/>等待刷新
2. 新事件不推迟首次刷新期限. Now: 连续结构事件<br/>已收到3次事件<br/>首次期限3，当前期限4<br/>等待刷新

With the fix, step 2 (新事件不推迟首次刷新期限) takes you to: 连续结构事件<br/>已收到3次事件<br/>首次期限3，当前期限3<br/>等待刷新.

## 3. 并发工具顺序<br/>工具二先完成<br/>展示顺序2、1

Rule broken: Reverse completion preserves admission order; mutators: crates/wunder-runtime/src/orchestrator/event_stream.rs:315; frontend/src/components/chat/MessageTimelineTurnProcess.vue:238.
Why: order.
In the model, one step leads into this bug: "逆序完成，不改变启动顺序".

0. Start: 并发工具顺序<br/>尚未启动<br/>展示顺序
1. 记录两次启动的稳定序号. Now: 并发工具顺序<br/>依次启动工具一、二<br/>展示顺序
2. 逆序完成，不改变启动顺序. Now: 并发工具顺序<br/>工具二先完成<br/>展示顺序2、1

With the fix, step 1 (记录两次启动的稳定序号) takes you to: 并发工具顺序<br/>依次启动工具一、二<br/>展示顺序1、2.
