## 1. 依附业务线程的网络驱动：业务与替补线程全部等待数据库

Rule broken: An unfinished database request must retain an enabled internal progress step.
Why: 网络驱动依附已耗尽的业务运行时.
In the model, one step leads into this bug: "调用同步数据库接口". The steps below are the shortest path that TLC found.

0. Start: 依附业务线程的网络驱动：业务等待 0，替补等待 0，响应就绪 0，完成 0
1. 调用同步数据库接口. Now: 依附业务线程的网络驱动：业务等待 1，替补等待 0，响应就绪 0，完成 0
2. 移交业务线程继续调度. Now: 依附业务线程的网络驱动：业务等待 0，替补等待 1，响应就绪 0，完成 0
3. 调用同步数据库接口. Now: 依附业务线程的网络驱动：业务等待 1，替补等待 1，响应就绪 0，完成 0
4. 移交业务线程继续调度. Now: 依附业务线程的网络驱动：业务等待 0，替补等待 2，响应就绪 0，完成 0
5. 调用同步数据库接口. Now: 依附业务线程的网络驱动：业务等待 1，替补等待 2，响应就绪 0，完成 0
6. 调用同步数据库接口. Now: 依附业务线程的网络驱动：业务与替补线程全部等待数据库

The fix cannot take these steps: it starts in another state.
