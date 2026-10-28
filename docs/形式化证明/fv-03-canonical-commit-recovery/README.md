# FV-03 canonical 提交、队列确认与崩溃恢复

日期：2026-10-26。对象：本地 canonical 存储的 create_thread / fork_known_source 提交顺序与 append 路径（`crates/wunder-runtime/src/storage/thread_store.rs`、`rollout.rs`）。不是全量崩溃一致性验收。

## 结论

`statechart` 对照确认：旧顺序先提交 `canonical_threads` 目录行、后写 SessionMeta 文件头（DB-first）。进程在两者之间崩溃留下"目录有行、rollout 文件从未创建"的线程；崩溃后第一次 append 由 writer 惰性建文件、首条记录的 SessionMeta 校验被跳过，从 ordinal 0 写出无头 rollout——`session_meta` 此后永久报 "rollout does not begin with session metadata"，且目录重名检查使重试 create 永远失败，线程不可恢复。两条规则（`NoMetadataWithoutHead`、`NoHeadlessRollout`）在旧模型均如期违反；修复模型（文件先行 + append 存在性防御）在同一配置下全部通过。真实修复已落地并在真实 SQLite/文件存储上回归。

这不是"create/fork 完全崩溃安全"的证明。模型是人工构造、与源码对应的有限抽象（1 个线程、1 次 create saga、崩溃只发生在持久步骤之间）；只有回归测试覆盖到的边界可以据此验收。

## 已确认的问题与修复

| 编号 | 原始路径与违反的规则 | 实际证据 | 修复 |
| --- | --- | --- | --- |
| 1 | create 先提交目录行再写文件，进程在两者之间崩溃留下有目录行、无文件的线程，且重名检查使重试永远失败 | 旧模型反例 3 步（CreateCatalog → Crash → 违反 `NoMetadataWithoutHead`）；基线 `thread_store.rs:2037-2047` 的 insert/append 顺序 | 反转为文件先行：先写 SessionMeta 头并 flush，再提交目录行（`create_thread` :2078-2110）；`fork_known_source` 同步反转（:1907-1937）。崩溃只遗留谱系无引用的孤儿文件，重试 create 用新的时间戳文件名成功 |
| 2 | 坏状态下第一次 append 经 `ensure_open(create=true)` 惰性建文件，首条非 SessionMeta 时头部校验被跳过，静默写出无头历史 | 旧模型反例 4 步（再接 Append → 违反 `NoHeadlessRollout`）；`rollout.rs:489-511` 惰性建文件 + :523-541 仅首条为 SessionMeta 时校验 | `append_records` 增加"目录行 ⇒ rollout 表示存在"防御（`thread_store.rs:2238-2256`）：plain/compressed 均缺失即返回 `CorruptHistory`，拒绝写入 |

补偿逻辑调整：新增 `compensate_new_thread_head`（`thread_store.rs:2020-2047`，不涉及 admission 行，目录/投影/文件删除均对"不存在"幂等），create 的 append 失败与目录插入失败两个失败点都调用；fork 补齐 shutdown 失败与目录插入失败两个点的补偿（`compensate_new_thread` 原函数不变，其对缺失行/文件的幂等性天然兼容文件先行）。

## 模型与编号路径

- `CanonicalCreate.tla` / `CanonicalCreate.cfg`：旧行为，行号参照基线提交 `508e12a`。
- `CanonicalCreateFixed.tla`：修复行为，行号参照本次修复后的源码；后续代码移动需更新引用。
- `CanonicalCreate.map.mjs`：把 TLC 状态翻译为读者语言。
- `model/summary.json` 是统计来源：旧模型 6 个状态、5 条迁移；合并对照图 11 个状态、13 条迁移；works 4、fixed 7、wrong 0、worse 0；两条规则在旧模型如期违反（3 步与 4 步反例各 1 条），修复模型全部满足；未标注源码位置的 guard 列表为空。
- `images/chart.png` 与 `model/chart.mmd` 是总图；`images/bug-1.png`、`images/bug-2.png` 和同名 Mermaid 是两条规则的编号路径；详细故事见 `model/stories.md`。

模型只覆盖两个持久事实（目录行、文件头）的提交顺序与一次崩溃；把 gate/owner 锁抽象为"create 与 append 不同时进行"的 pc 约束，不建模磁盘掉电（flush ≠ sync_all 属设计内语义，见对齐方案 §7.3）、多线程并发 create、压缩/归档交错；限界为 1 个线程、1 次 create saga。

## 真实回归

回归入口是 `crates/wunder-runtime/src/storage/thread_store.rs` 测试模块的三个新测试：

- `create_interrupted_after_head_write_leaves_orphan_and_retry_succeeds`：模拟文件先行 create 在目录提交前崩溃（直接对文件写 SessionMeta 头、不插目录行）；断言读路径报 NotFound、重试 create（新 rollout id）成功、后续 append 正常、孤儿文件留存但不进入谱系。
- `catalog_row_without_rollout_file_is_detected_and_append_rejected`：直接构造旧坏状态（`insert_thread_metadata` 提交目录行、文件从未创建）；断言 `session_meta` 报错、append 返回 `CorruptHistory`、文件仍未被创建（无无头 rollout）、`delete_thread` 可正常清理。
- `fork_interrupted_after_head_write_leaves_orphan_and_retry_succeeds`：fork 同款窗口（带头孤儿文件 + 无目录行）；重试 `fork_through_turn` 成功，`history_base` 指向源 rollout，谱系深度 2，孤儿文件不受影响。

执行结果：三个新测试通过；`storage::thread_store` 模块全量 25/25 通过（含既有删除 saga、回退恢复、fork/共享前缀、压缩谱系重建、压测重放等回归）；`storage::` 子树全量 139 通过、0 失败、1 显式忽略。

## 复现命令

在仓库根目录执行测试：

```powershell
cargo check -p wunder-runtime --features sqlite-storage --tests
cargo test -p wunder-runtime --features sqlite-storage --lib -- interrupted catalog_row_without
cargo test -p wunder-runtime --features sqlite-storage --lib -- storage::thread_store
```

模型需已安装 statechart skill；用已有 Java 与经验证的 TLC 设置 `JAVA` 和 `TLA2TOOLS`，无需在仓库中保存 Java/JAR。运行输出放仓库外：

```powershell
$proof = 'docs/形式化证明/fv-03-canonical-commit-recovery'
$skill = Join-Path $env:USERPROFILE '.codex/skills/statechart/scripts/statechart.mjs'
$out = Join-Path $env:TEMP ('fv-03-' + [guid]::NewGuid().ToString('N'))
node $skill from-spec "$proof/CanonicalCreate.tla" --fixed "$proof/CanonicalCreateFixed.tla" --map "$proof/CanonicalCreate.map.mjs" --expect-broken NoMetadataWithoutHead --expect-broken NoHeadlessRollout --out $out --cover --png
```

## 相关实现文件

- `crates/wunder-runtime/src/storage/thread_store.rs`：`create_thread` :2051（文件先行 :2078-2110）、`fork_known_source` :1861（:1907-1937）、`append_records` :2217（存在性防御 :2238-2256）、`compensate_new_thread_head` :2020、`compensate_new_thread` :1984
- `crates/wunder-runtime/src/storage/rollout.rs`（本次未修改）：惰性建文件 :457-515、首条 SessionMeta 校验 :523-541、逐条 write+flush :565-575、重开截断撕裂尾部 :771-801
- `crates/wunder-runtime/src/storage/thread_mutation.rs`（未修改）：revert 的 intent + `prepare_suffix` 已是意图先行，本窗口不涉及
- `docs/对齐codex存储设计方案.md` §7.2/§7.4/§7.5：提交顺序、故障窗口与跨库 saga 的契约依据

## 尚未证明的部分

- 断电耐久性：活跃记录只有逐条 write+flush、无 fsync（设计明确不承诺抗突然断电，对齐方案 §7.3），未建模也未测试。
- 目录行引用缺失 rollout 的检测只覆盖 append 入口；`flush`/`recover` 对该状态仍返回空水位的 Ok（不产生写入，但可能掩盖坏状态）。
- 孤儿文件无回收入口：无启动扫描属设计内决策，本次未提供显式清理工具，孤儿文件只做无害留存断言。
- 多线程并发 create、压缩/归档与 create 交错、以及 PostgreSQL 服务形态（服务端 canonical 在单事务内提交，不存在本窗口）均不在本模型内。
- `fork` 的 queue 半边（`copy_prefix`）崩溃窗口：其失败补偿已存在并保留，本次未单独建模。
