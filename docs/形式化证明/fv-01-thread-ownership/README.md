# FV-01 线程打开权、执行权与过期持有者隔离

日期：2026-10-26。对象：共享引擎的线程打开租约（`thread_open_leases`）与执行锁（`session_locks`），SQLite 与 PostgreSQL 双存储实现。不是全量并发验收。

## 结论

`statechart` 对照确认：打开租约获得时即带 fencing token（此前已修），而执行锁的 touch/release 只按 `session_id` 匹配——持有者心跳停顿到锁过期、被接管后，旧持有者恢复时的迟到释放会删除新持有者的锁行，迟到心跳会给新持有者的锁行续期。两条规则在旧模型均如期违反，修复模型（touch/release 的 WHERE 增加 `lock_token` 代次匹配）同一配置下全部通过。真实修复已落地并在 SQLite 真实存储上回归。

这不是"执行权完全隔离"的证明。模型是人工构造、与源码对应的有限抽象（两个执行者、各 2 代限界）；只有回归测试覆盖到的边界可以据此验收。

## 已确认的问题与修复

| 编号 | 原始路径与违反的规则 | 实际证据 | 修复 |
| --- | --- | --- | --- |
| 1 | A 持执行锁 → 心跳停顿锁过期 → B 接管获新行 → A 执行结束迟到 `release` 只按 session_id 匹配，删掉 B 的锁行 | 旧模型反例 16 条（最短 8 步）；基线 `session_lock_store.rs:130-134` DELETE 无代次条件 | `session_locks` 增加 `lock_token` 列，获得时颁发 uuid；release 的 WHERE 带 `session_id + lock_token` |
| 2 | 同上交错，A 恢复心跳后迟到 `touch` 给 B 的锁行续期 | 旧模型反例 8 条（最短 8 步）；基线 `session_lock_store.rs:117-120` UPDATE 无代次条件 | touch 的 WHERE 带 `session_id + lock_token`；失权心跳返回 false，执行侧打 warn |

## 模型与编号路径

- `ThreadOwnership.tla` / `ThreadOwnership.cfg`：旧行为，行号参照基线提交 `508e12a`。
- `ThreadOwnershipFixed.tla`：修复行为，行号参照本次修复后的源码；后续代码移动需更新引用。
- `ThreadOwnership.map.mjs`：把 TLC 状态翻译为读者语言。
- `model/summary.json` 是统计来源：旧模型 349 个状态、864 条迁移；合并对照图 117 个状态、387 条迁移；works 76、fixed 41、wrong 0、worse 0；两条规则在旧模型如期违反，修复模型全部满足；未标注源码位置的 guard 列表为空。
- `images/chart.png` 与 `model/chart.mmd` 是总图；`images/bug-1.png`、`images/bug-2.png` 和同名 Mermaid 是两条规则的编号路径；详细故事见 `model/stories.md`。

模型把 TTL 抽象为布尔 alive（时延另测），不建模 UserBusy 自旋、SystemBusy 限流、执行内的实际工作；限界为 2 个执行者、打开租约与执行锁各最多 2 代。TTL 过期到接管之间的物理并发执行属设计内（锁过期即失权，不产生新副作用的校验归 FV-04）。

## 真实回归

回归入口是 `crates/wunder-runtime/src/storage/sqlite/session_lock_store.rs` 的
`stale_holder_late_release_and_touch_cannot_reach_the_new_generation`：两个 `SqliteStorage` 实例共享同一数据库文件（等价两个本地进程），A 获得执行锁后直接 SQL 强制过期，B 接管获得新代次；断言 A 的迟到 touch 返回 false 且不改变 B 行的 expires_at，A 的迟到 release 后 B 行仍在，B 自己的 touch/release 正常生效。

打开租约侧的旧代次拒绝由既有测试覆盖：`thread_open_lease_store.rs` 的 `expired_same_owner_cannot_reuse_old_generation`（过期后旧代次 touch/release 失配、`accept_thread_turn` 拒绝旧代次）与 `execution_lock_blocks_new_open_lease`（活跃执行锁阻止新打开租约）。

PostgreSQL 实现（`storage/postgres/session_lock_store.rs`）与 SQLite 同语义同列结构，本次仅编译期验证；运行时验证需要 `WUNDER_TEST_POSTGRES_DSN`，待补。

## 复现命令

在仓库根目录执行测试：

```powershell
cargo test -p wunder-runtime --features sqlite-storage --lib storage::sqlite::session_lock_store
cargo test -p wunder-runtime --features sqlite-storage --lib thread_open_lease
```

模型需已安装 statechart skill；也可以用已有 Java 与经验证的 TLC 设置 `JAVA` 和 `TLA2TOOLS`，无需在仓库中保存 Java/JAR。运行输出放仓库外：

```powershell
$proof = 'docs/形式化证明/fv-01-thread-ownership'
$skill = Join-Path $env:USERPROFILE '.codex/skills/statechart/scripts/statechart.mjs'
$out = Join-Path $env:TEMP ('fv-01-' + [guid]::NewGuid().ToString('N'))
node $skill from-spec "$proof/ThreadOwnership.tla" --fixed "$proof/ThreadOwnershipFixed.tla" --map "$proof/ThreadOwnership.map.mjs" --expect-broken LateReleaseKeepsLock --expect-broken LateTouchKeepsLock --out $out --cover --png
```

## 相关实现文件

- `crates/wunder-runtime/src/storage/sqlite/session_lock_store.rs`（touch :129、release :144 的代次匹配；获得 :45 颁发 uuid）
- `crates/wunder-runtime/src/storage/postgres/session_lock_store.rs`（同语义）
- `crates/wunder-runtime/src/storage/sqlite/schema.rs`（`ensure_session_locks_column` 幂等补列）、`schema_queue.sql`
- `crates/wunder-runtime/src/orchestrator/limiter.rs`（acquire 返回 token，touch/release 携带）
- `crates/wunder-runtime/src/orchestrator/execute.rs:257-272`（心跳携带 token，失权打 warn）
- `crates/wunder-runtime/src/orchestrator/execute_tools.rs`（`finish_request_resources` 按 token 释放）
- `crates/wunder-runtime/src/services/workspace.rs`（管理面强删走 `force_release_session_lock`）

## 尚未证明的部分

- 打开租约接管竞争本身（本模型中打开租约已 fenced，未对其建模反例；接管回归见既有测试）。
- 旧执行失权后的终态写入拦截（提交边界校验归 FV-04）；本项只保证锁行不被误删/误续。
- TTL 过期窗口内两个执行者的物理并发（设计内失权语义，不代表无副作用冲突）。
- PostgreSQL 运行时行为、多实例跨机时钟偏差下的 TTL 一致性、任意数量执行者的并发。
