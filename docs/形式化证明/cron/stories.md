## 1. 任务启用，2个执行尚未结束，重复领取

Rule broken: 租约未失效时同一任务不得重复获得执行权。cron.rs:557; sqlite/cron.rs:294
Why: 未失效的执行权被再次领取.
In the model, one step leads into this bug: "手动请求尝试领取". The steps below are the shortest path that TLC found.

0. Start: 任务启用，无在途执行
1. 手动请求读取任务. Now: 任务启用，手动请求已读取，无在途执行
2. 定时调度原子领取. Now: 任务启用，手动请求已读取，1个执行尚未结束
3. 手动请求尝试领取. Now: 任务启用，2个执行尚未结束，重复领取

With the fix, step 3 (手动请求尝试领取) takes you to: 任务启用，1个执行尚未结束.

## 2. 任务启用，1个执行尚未结束

Rule broken: 删除完成后，迟到的领取与完成回写不得复活任务。cron.rs:557; crates/wunder-runtime/src/services/cron.rs:2385
Why: 删除后仍用旧快照重新插入.
In the model, 2 different steps lead into this bug: "提交完成状态" and "手动请求尝试领取". The steps below are the shortest path that TLC found.

0. Start: 任务启用，无在途执行
1. 手动请求读取任务. Now: 任务启用，手动请求已读取，无在途执行
2. 删除任务. Now: 任务已删除，手动请求已读取，无在途执行
3. 手动请求尝试领取. Now: 任务启用，1个执行尚未结束

With the fix, step 3 (手动请求尝试领取) takes you to: 任务已删除，无在途执行.

## 3. 任务停用，1个执行尚未结束，等待完成提交，历史与状态分离

Rule broken: 运行历史不得在删除或换代后单独写入。sqlite/cron.rs:100; crates/wunder-runtime/src/storage/postgres/cron.rs:96
Why: 历史写入没有绑定有效执行权.
In the model, one step leads into this bug: "单独写入执行历史". The steps below are the shortest path that TLC found.

0. Start: 任务启用，无在途执行
1. 定时调度原子领取. Now: 任务启用，1个执行尚未结束
2. 执行结束读取状态. Now: 任务启用，1个执行尚未结束，等待完成提交
3. 停用后续调度. Now: 任务停用，1个执行尚未结束，等待完成提交
4. 单独写入执行历史. Now: 任务停用，1个执行尚未结束，等待完成提交，历史与状态分离

With the fix, step 4 (单独写入执行历史) cannot happen, so you stay at: 任务停用，1个执行尚未结束，等待完成提交.

## 4. 任务启用，无在途执行，覆盖新状态

Rule broken: 完成回写不得覆盖并发编辑、停用或新的租约。cron.rs:2385
Why: 旧执行覆盖了新的任务状态.
In the model, one step leads into this bug: "提交完成状态". The steps below are the shortest path that TLC found.

0. Start: 任务启用，无在途执行
1. 定时调度原子领取. Now: 任务启用，1个执行尚未结束
2. 执行结束读取状态. Now: 任务启用，1个执行尚未结束，等待完成提交
   (1 hidden step: 单独写入执行历史)
3. 停用后续调度. Now: 任务停用，1个执行尚未结束，等待完成提交
4. 提交完成状态. Now: 任务启用，无在途执行，覆盖新状态

With the fix, step 4 (提交完成状态) takes you to: 任务停用，1个执行尚未结束.
