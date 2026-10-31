## 1. 预算耗尽，页面为空，尚未返回条目

Rule broken: A trimmed turn that still has canonical items never returns an empty page.
Why: 从稀疏锚点起步，预算在到达轮次条目之前耗尽.
In the model, one step leads into this bug: "记录预算在轮次前耗尽".

0. Start: 等待打开较早轮次，尚未返回条目
1. 打开活动窗口外的轮次. Now: 读取 canonical 记录，尚未返回条目
2. 逐条读取 rollout 记录. Now: 读取 canonical 记录，尚未返回条目
3. 逐条读取 rollout 记录. Now: 读取 canonical 记录，尚未返回条目
4. 逐条读取 rollout 记录. Now: 读取 canonical 记录，尚未返回条目
5. 逐条读取 rollout 记录. Now: 读取 canonical 记录，尚未返回条目
6. 记录预算在轮次前耗尽. Now: 预算耗尽，页面为空，尚未返回条目

With the fix, step 4 (逐条读取 rollout 记录) takes you to: 读取 canonical 记录，已收集 1 项.
