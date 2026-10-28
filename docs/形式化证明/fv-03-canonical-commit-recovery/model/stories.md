## 1. 目录行已发布<br/>rollout 文件不存在<br/>无进行中的 create

Rule broken: 规则1：静止状态（无 create saga 在飞，即崩溃后可观察到的任一时刻）下， 目录有行的线程其当前 rollout 必须已含 SessionMeta 头。 变更点：thread_store.rs:2037-2047 先插目录行、后写文件的顺序。
Why: create 先提交目录行再写文件，进程在两者之间崩溃留下有目录行、无文件的线程，且重名检查使重试永远失败.
In the model, one step leads into this bug: "进程崩溃：目录行已提交但 rollout 文件从未创建".

0. Start: 无目录行<br/>rollout 文件不存在<br/>无进行中的 create
1. 在 Immediate 事务中提交 canonical_threads 目录行. Now: 目录行已发布<br/>rollout 文件不存在<br/>目录行已提交、头未写
2. 进程崩溃：目录行已提交但 rollout 文件从未创建. Now: 目录行已发布<br/>rollout 文件不存在<br/>无进行中的 create

With the fix, step 1 (在 Immediate 事务中提交 canonical_threads 目录行) cannot happen, so you stay at: 无目录行<br/>rollout 文件不存在<br/>无进行中的 create.

## 2. 目录行已发布<br/>rollout 首条是普通记录（无头历史）<br/>无进行中的 create<br/>已发生后续 append

Rule broken: 规则2：任何 rollout 文件的首条记录必须是 SessionMeta，不允许无头历史。 变更点：rollout.rs:489-511 惰性建文件 + :523-541 仅首条为 SessionMeta 时校验。
Why: 缺失文件被 writer 惰性创建，且 SessionMeta 校验只在首条本身是 SessionMeta 时执行，普通记录从 ordinal 0 接续成无头历史.
In the model, one step leads into this bug: "崩溃后第一次 append：writer 惰性创建文件，从 ordinal 0 写出无头 rollout".

0. Start: 无目录行<br/>rollout 文件不存在<br/>无进行中的 create
1. 在 Immediate 事务中提交 canonical_threads 目录行. Now: 目录行已发布<br/>rollout 文件不存在<br/>目录行已提交、头未写
2. 进程崩溃：目录行已提交但 rollout 文件从未创建. Now: 目录行已发布<br/>rollout 文件不存在<br/>无进行中的 create
3. 崩溃后第一次 append：writer 惰性创建文件，从 ordinal 0 写出无头 rollout. Now: 目录行已发布<br/>rollout 首条是普通记录（无头历史）<br/>无进行中的 create<br/>已发生后续 append

With the fix, step 1 (在 Immediate 事务中提交 canonical_threads 目录行) cannot happen, so you stay at: 无目录行<br/>rollout 文件不存在<br/>无进行中的 create.
