## 1. 气泡完成，旧工具转圈<br/>已有过程页

Rule broken: A completed latest page cannot settle with a stale running tool and no scheduled reread. Mutators: timeline_text.rs:413; tool_process.slint:186, :191; chat_process.rs:182.
Why: 终态保留旧过程页，运行刷新定时器却已经停止.
In the model, one step leads into this bug: "完成历史替换运行历史".

0. Start: 气泡运行，旧工具转圈<br/>已有过程页
1. 完成历史替换运行历史. Now: 气泡完成，旧工具转圈<br/>已有过程页

With the fix, step 1 (完成历史替换运行历史) takes you to: 气泡完成，工具页待重读<br/>尚未加载过程页.
