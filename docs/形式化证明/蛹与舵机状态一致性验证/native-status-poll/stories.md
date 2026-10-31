## 1. 实际 A 已完成、B 已完成；条目 A 已完成、B 运行中；选中 A；窗口可见

Rule broken: Once nothing else can act and the sidebar is owned, a thread that has settled must not still read as running.
Why: B 在后台结束，但目录刷新只由被选中线程的观察器触发，B 的条目停在运行中，直到读者切回 B.
In the model, 7 different steps lead into this bug: "窗口重新拥有侧栏", "A 的观察器把 A 的实际相位写入 A 的条目", "B 的观察器把 B 的实际相位写入 B 的条目", "切换到线程 A", "切换到线程 B", and 2 more. The steps below are the shortest path that TLC found.

0. Start: 实际 A 已完成、B 运行中；条目 A 已完成、B 运行中；选中 A；窗口可见
1. 线程 B 在后台跑完（无本地操作）. Now: 实际 A 已完成、B 已完成；条目 A 已完成、B 运行中；选中 A；窗口可见

With the fix you see the same states and steps, yet the rule holds, so the state labels do not show what the fix changes.
