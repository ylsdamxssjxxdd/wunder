--------------------------- MODULE CanonicalCreate ---------------------------
\* FV-03 旧行为模型：create_thread / fork 的 DB-first 提交顺序与崩溃窗口。
\* 基线提交 508e12a。限界：一个任务线程、一次 create saga（可崩溃后重试）。
\* 两个持久事实：canonical_threads 目录行（catalog）与当前 rollout 文件（file）。
\* file 取值：absent=文件从未创建；head=首条为 SessionMeta；headless=首条为普通记录
\* （崩溃后第一次 append 由 writer 惰性建文件，rollout.rs:523-541 只在首条记录
\* 本身是 SessionMeta 时才校验头部，普通记录从 ordinal 0 直接接续成无头历史）。
EXTENDS Integers
VARIABLES catalog, file, orphan, pc, appended
vars == <<catalog, file, orphan, pc, appended>>

Init ==
    /\ catalog = "absent"
    /\ file = "absent"
    /\ orphan = FALSE
    /\ pc = "idle"
    /\ appended = FALSE

\* create_thread 第一步：insert_thread_metadata 在 Immediate 事务中提交目录行。
\* thread_store.rs:2037-2038 调用 :1944-1972（:1966 重名检查、:1969 提交）。
CreateCatalog ==
    /\ pc = "idle"
    \* thread_store.rs:1966：目录行或删除标记存在则拒绝，重试永远失败
    /\ catalog = "absent"
    /\ catalog' = "present"
    /\ pc' = "db"
    /\ UNCHANGED <<file, orphan, appended>>

\* create_thread 第二步：append_records 写入 SessionMeta 头并逐条 flush。
\* thread_store.rs:2041-2047；rollout.rs:565-575（write_all + flush，非 fsync）。
CreateHead ==
    /\ pc = "db"
    /\ file' = "head"
    /\ pc' = "done"
    /\ UNCHANGED <<catalog, orphan, appended>>

\* 进程在目录行提交与文件写入之间崩溃：补偿只覆盖进程内的错误返回
\* （thread_store.rs:2049-2074），进程崩溃不触发任何清理；
\* 目录行与"从未创建的文件"一起永久留存，重名检查使重试 create 永远失败。
Crash ==
    /\ pc = "db"
    /\ pc' = "idle"
    /\ UNCHANGED <<catalog, file, orphan, appended>>

\* 崩溃后的第一次 append：append_records（thread_store.rs:2187-2266）只要求目录行
\* 存在且未归档（:2202-2207）；active_writer 经 rollout.rs:457-515
\* ensure_open(create=true) 创建缺失文件，:523-541 的 SessionMeta 校验因首条是
\* 普通记录而跳过，从 ordinal 0 写出无头 rollout；session_meta 此后永远报
\* "rollout does not begin with session metadata"（thread_store.rs:582）。
Append ==
    \* create 全程持有线程 gate 与 owner 锁（thread_store.rs:2027-2029）
    /\ catalog = "present"
    \* append 与 create 经同一线程 gate 互斥（thread_store.rs:2199-2200）
    /\ pc \in {"idle", "done"}
    /\ file # "headless"
    /\ appended' = TRUE
    /\ IF file = "absent"
       THEN file' = "headless"
       ELSE UNCHANGED file
    /\ UNCHANGED <<catalog, orphan, pc>>

Next == CreateCatalog \/ CreateHead \/ Crash \/ Append

TypeOK ==
    /\ catalog \in {"absent", "present"}
    /\ file \in {"absent", "head", "headless"}
    /\ orphan \in BOOLEAN
    /\ pc \in {"idle", "db", "done"}
    /\ appended \in BOOLEAN

\* 规则1：静止状态（无 create saga 在飞，即崩溃后可观察到的任一时刻）下，
\* 目录有行的线程其当前 rollout 必须已含 SessionMeta 头。
\* 变更点：thread_store.rs:2037-2047 先插目录行、后写文件的顺序。
NoMetadataWithoutHead ==
    (pc = "idle" /\ catalog = "present") => file = "head"

\* 规则2：任何 rollout 文件的首条记录必须是 SessionMeta，不允许无头历史。
\* 变更点：rollout.rs:489-511 惰性建文件 + :523-541 仅首条为 SessionMeta 时校验。
NoHeadlessRollout == file # "headless"

Spec == Init /\ [][Next]_vars
=============================================================================
