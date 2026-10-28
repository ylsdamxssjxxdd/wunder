## 1. 持久祖先已取消<br/>子任务：执行中<br/>执行代次：1<br/>取消后仍派生了新任务

Rule broken: A cancelled durable ancestor forbids sibling dispatch into its scope.
Why: 只核对直接调度者，遗漏持久派生树中的已取消祖先.
In the model, one step leads into this bug: "兄弟调度器派生祖先下的任务".

0. Start: 持久祖先允许派生<br/>子任务：未派生<br/>执行代次：0
1. 取消祖先及已登记后代. Now: 持久祖先已取消<br/>子任务：未派生<br/>执行代次：0
2. 兄弟调度器派生祖先下的任务. Now: 持久祖先已取消<br/>子任务：执行中<br/>执行代次：1<br/>取消后仍派生了新任务

With the fix, step 2 (兄弟调度器派生祖先下的任务) cannot happen, so you stay at: 持久祖先已取消<br/>子任务：未派生<br/>执行代次：0.
