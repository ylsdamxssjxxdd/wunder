## 1. 本地任务已完成<br/>目录记录已完成，已弹出完成提示

Rule broken: A previous catalogue snapshot cannot finish a locally submitted turn or show its notice. Old behaviour: e5a90a38 frontend-slint/src/native_chat.rs:1476 observed every returned row.
Why: 目录读取开始早于本地提交，旧完成快照覆盖本地热状态.
In the model, one step leads into this bug: "旧目录快照返回完成".

0. Start: 本地任务已完成<br/>目录记录已完成，未弹完成提示
1. 用户发送消息. Now: 本地任务执行中<br/>目录记录已完成，未弹完成提示
2. 旧目录快照返回完成. Now: 本地任务已完成<br/>目录记录已完成，已弹出完成提示

With the fix, step 2 (旧目录快照返回完成) keeps you at: 本地任务执行中<br/>目录记录已完成，未弹完成提示.
