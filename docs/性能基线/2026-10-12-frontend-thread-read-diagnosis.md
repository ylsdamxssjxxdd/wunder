# 2026-10-12 蜂巢线程读取与渲染链路诊断

## 结论与边界

当前重构已加入分页上限、兜底、分片、缓存和轨迹节流，但仍可用匿名小样本复现“非空快照生成零行”“创建空 runtime 后已有兜底消失”“恢复取消破坏已显示状态”。固定返回页的 SQL 扫描成本和缓存命中时的投影成本仍随历史规模增长。

本报告为方案重写前的诊断基线，**未修改业务实现，未验证性能提升**。没有真实浏览器/部署数据库前后对比；下列耗时来自内存 SQLite 与 Node，只用来识别查询形状和计算增长，不代表线上首屏时延。方案 v3 的一秒首屏目标仍待实现与真实端到端验收。

## 环境与数据

- 平台：Windows，32 个逻辑处理器。
- Node：v24.14.0；Python：3.12.10；SQLite：3.49.1。
- 使用当前工作区 TypeScript 源码，通过 esbuild 在内存中打包诊断；使用匿名生成的数据，无真实账号、正文、路径或密钥。
- SQLite 诊断从当前 thread_snapshot_impl 提取 item SELECT，建立与查询相关的主键、唯一键、root/item 与 created_seq 部分索引；不读取工作区实际数据库。
- Node 投影时间只测缓存预热后的相同状态，3 次采样。完整快照为 4 个 root、每 root 1 条用户 item + 40 条模型文本 item。
- SQLite 固定选最近 2 个 turn、返回 20 条 item；总量 20000/200000/500000，分别测 3 次。所有 fixture item 为通用工具记录，明确暴露原始快照过滤与索引形状的成本。
- SQLite 无磁盘、网络、payload 解析或文本块查询成本。PostgreSQL 未运行；相关结论不外推为双库生产耗时。

## 复现入口

仓库根执行：

~~~text
node scripts/diagnostics/2026-10-12-frontend-thread-read-probe.mjs
python scripts/diagnostics/2026-10-12-runtime-thread-query-probe.py
~~~

frontend 目录执行：

~~~text
node scripts/run-regression-tests.mjs chat-transcript-fallback-regression.test.ts chat-thread-hydrate-shard-regression.test.ts chat-thread-projection-memo-regression.test.ts
~~~

诊断脚本仅打印当前行为，不作为“旧实现通过”的质量门禁。SQL 结构变化后脚本会提示更新诊断；新实现的正确性与性能门禁按方案 §9 建立。

## 正确性诊断

| 情况 | 当前结果 | 含义 |
| --- | --- | --- |
| 完整 4 turn / 164 item | 4 个 slot | 标准形状可显示 |
| 保留 turn，只返回最新 20 个 item | 0 个 slot，cursor=200 | root 输入未从 payload 恢复，item cap 排掉用户记录 |
| runtime 缺 entry，transcript 有一组消息 | 1 个兜底轮次 | 已有消息可以显示 |
| watcher 等代码仅创建空 entry | 零行、activity_card=true | entry 存在被错误用作展示数据可用证明 |
| 完整状态被截断快照覆盖 | 4 行→0 行、activity_card=true | 有内容后回到欢迎卡的可复现条件 |
| 分片恢复第一个 shard 后取消 | 仅 1 turn / 0 item，旧 cursor=200 | 原有页状态已被清除，cursor 没推进不能保证事务性 |

这是调用真实 reducer、projection、runtime materializer 与兜底仲裁函数得到的状态输出。未运行页面导航，因此没有宣称已复现真实浏览器全部路由变化。

## 成本诊断

| 固定查询页，总 item 数 | 返回 item | 三次查询中位数 |
| --- | ---: | ---: |
| 20000 | 20 | 2.72ms |
| 200000 | 20 | 24.18ms |
| 500000 | 20 | 58.57ms |

SQLite 计划只按 user_id/session_id 使用 idx_thread_items_turn，然后过滤 turn_id 并临时排序。存在 LIMIT 不能证明 rows visited 有界；输出量固定而耗时随总量增长。

| 未变化的投影状态 | item 数 | 缓存命中三次中位数 |
| --- | ---: | ---: |
| 20 root 轮次 | 2020 | 5.90ms |
| 200 root 轮次 | 20200 | 50.49ms |

slot 对象身份复用成功，但缓存判定本身仍扫描整窗并组合文本 signature。Node 结果只说明该计算在缓存命中时没有消失；浏览器 JSON、Vue、DOM 与布局开销需另外采样。

## 回归结果与覆盖缺口

现有 transcript fallback 11 项、分片 hydrate 6 项、projection memo 4 项，**21 项全部通过**。同一源码仍出现上述诊断输出。

通过项证明既有 helper 的预设语义成立，不证明真实用户页面性能或状态正确。覆盖缺口包括：把空 registry 等同真实空线程；分片取消只检查 cursor 不前进；缓存测试只检查 identity，没有约束遍历/文本成本；旧性能样张固定记录耗时，没有真实首屏链路。后续门禁应改变这些错误前提。

## 原始诊断输出

Node 本次复跑：

~~~json
{
  "scope": "synthetic Node diagnostics; not production browser latency",
  "snapshot": {
    "complete": {
      "turns": 4,
      "items": 164,
      "slots": 4
    },
    "capped": {
      "turns": 4,
      "items": 20,
      "slots": 0,
      "cursor": 200
    }
  },
  "render_source": {
    "absent": {
      "source": "transcript_fallback",
      "rows": 1,
      "activity_card": false
    },
    "allocated": {
      "source": "empty",
      "rows": 0,
      "activity_card": true
    },
    "complete": {
      "source": "window",
      "rows": 4,
      "activity_card": false
    },
    "truncated": {
      "source": "empty",
      "rows": 0,
      "activity_card": true
    }
  },
  "interrupted_recovery": {
    "outcome": {
      "applied": false,
      "aborted": true,
      "shardsApplied": 1,
      "shardCount": 168,
      "yields": 1
    },
    "turns": 1,
    "items": 0,
    "cursor": 200
  },
  "cache_hit_timing": [
    {
      "roots": 20,
      "items": 2020,
      "unchanged_projection_ms": [
        5.9,
        6.26,
        4.47
      ]
    },
    {
      "roots": 200,
      "items": 20200,
      "unchanged_projection_ms": [
        53.79,
        46.25,
        50.49
      ]
    }
  ]
}
~~~

SQLite 本次复跑：

~~~json
{
  "scope": "synthetic in-memory SQLite query shape; not production latency",
  "sqlite_version": "3.49.1",
  "query_plan": [
    "SEARCH thread_items USING INDEX idx_thread_items_turn (user_id=? AND session_id=?)",
    "LIST SUBQUERY 1",
    "SEARCH thread_turns USING INDEX idx_thread_turns_page (user_id=? AND session_id=? AND user_turn_index<?)",
    "USE TEMP B-TREE FOR LAST 2 TERMS OF ORDER BY",
    "USE TEMP B-TREE FOR ORDER BY"
  ],
  "results": [
    {
      "total_items": 20000,
      "window_turns": 2,
      "returned_items": 20,
      "query_ms": [
        2.72,
        3.05,
        2.2
      ]
    },
    {
      "total_items": 200000,
      "window_turns": 2,
      "returned_items": 20,
      "query_ms": [
        24.18,
        23.65,
        28.18
      ]
    },
    {
      "total_items": 500000,
      "window_turns": 2,
      "returned_items": 20,
      "query_ms": [
        58.84,
        58.31,
        58.57
      ]
    }
  ]
}
~~~

## 后续实施

按 [方案 v3](../蜂巢大数据量线程日志渲染优化方案.md) 建立事务内展示投影、单请求 bootstrap、独立候选恢复、正文分块、多级轨迹摘要与真实入口门禁。未实施前，不把本报告或旧 helper 测试通过登记为性能提升。

