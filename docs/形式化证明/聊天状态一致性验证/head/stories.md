## 1. 线程完成，气泡仍运行

Rule broken: A completed thread entry must not coexist with its still-running bubble. Mutators: threadViewStore.ts:350, threadViewStore.ts:394.
Why: 目录只推进线程头，没有同时更新对应气泡.
In the model, one step leads into this bug: "目录先返回完成态".

0. Start: 尚未发送
1. 受理后提交展示窗. Now: 线程与气泡运行
2. 目录先返回完成态. Now: 线程完成，气泡仍运行

With the fix, step 2 (目录先返回完成态) takes you to: 线程与气泡完成.
