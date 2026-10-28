## 1. 单条事件超预算，元信息不复制正文，统计不扫描历史，空投影

Rule broken: Per-event diagnostics have a byte budget, not merely a slot limit.
Why: 限制条数未限制单条正文.
In the model, one step leads into this bug: "收到模型或工具事件". The steps below are the shortest path that TLC found.

0. Start: 诊断在预算内，元信息不复制正文，统计不扫描历史，空投影
1. 收到模型或工具事件. Now: 单条事件超预算，元信息不复制正文，统计不扫描历史，空投影

With the fix, step 1 (收到模型或工具事件) keeps you at: 诊断在预算内，元信息不复制正文，统计不扫描历史，空投影.

## 2. 诊断在预算内，元信息不复制正文，统计扫描历史，已有投影

Rule broken: Hot count reads inspect no historical item rows after initialization.
Why: 热查询重新扫描历史条目.
In the model, one step leads into this bug: "舰桥读取条目统计". The steps below are the shortest path that TLC found.

0. Start: 诊断在预算内，元信息不复制正文，统计不扫描历史，空投影
1. 原子写入或重放投影. Now: 诊断在预算内，元信息不复制正文，统计不扫描历史，已有投影
2. 舰桥读取条目统计. Now: 诊断在预算内，元信息不复制正文，统计扫描历史，已有投影

With the fix, step 2 (舰桥读取条目统计) keeps you at: 诊断在预算内，元信息不复制正文，统计不扫描历史，已有投影.

## 3. 诊断在预算内，元信息复制正文，统计不扫描历史，空投影

Rule broken: Metadata reads copy no diagnostic body under the global monitor mutex.
Why: 元信息读取复制诊断正文.
In the model, one step leads into this bug: "运行时读取元信息". The steps below are the shortest path that TLC found.

0. Start: 诊断在预算内，元信息不复制正文，统计不扫描历史，空投影
1. 收到模型或工具事件. Now: 诊断在预算内，元信息不复制正文，统计不扫描历史，空投影
2. 运行时读取元信息. Now: 诊断在预算内，元信息复制正文，统计不扫描历史，空投影

With the fix, step 2 (运行时读取元信息) keeps you at: 诊断在预算内，元信息不复制正文，统计不扫描历史，空投影.
