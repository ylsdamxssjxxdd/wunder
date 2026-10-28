------------------------ MODULE CanonicalCreateFixed ------------------------
\* FV-03 修复模型：create_thread / fork 反转为文件先行——先写 SessionMeta 头并
\* flush，再提交目录行；进程在两者之间崩溃只遗留谱系无引用的孤儿文件，
\* 目录从未发布的线程对读路径不可见，重试 create 用新的时间戳文件名成功。
\* append_records 增加"目录行 ⇒ rollout 表示存在"的防御：旧坏状态被检测并拒绝，
\* 不再静默写出无头历史。行号参照本次修复后的源码。
EXTENDS Integers
VARIABLES catalog, file, orphan, pc, appended, rejected
vars == <<catalog, file, orphan, pc, appended, rejected>>

Init ==
    /\ catalog = "absent"
    /\ file = "absent"
    /\ orphan = FALSE
    /\ pc = "idle"
    /\ appended = FALSE
    /\ rejected = FALSE

\* 修复后第一步：RolloutRecorder::create 只定文件名不碰文件系统
\* （rollout.rs:243-269），append_records 建目录与文件、写 SessionMeta 并 flush
\* （thread_store.rs:2083-2090；rollout.rs:565-575）。此刻目录行不存在，
\* 崩溃只留孤儿文件。
CreateHead ==
    /\ pc = "idle"
    \* 目录重名由 insert 事务兜底（thread_store.rs:1976），重复 create 先写文件
    \* 再由补偿删除；create 全程持有线程 gate 与 owner 锁（:2068-2070）
    /\ catalog = "absent"
    /\ file = "absent"
    /\ file' = "head"
    /\ pc' = "file"
    /\ UNCHANGED <<catalog, orphan, appended, rejected>>

\* 修复后第二步：头部已持久才在同一 Immediate 事务发布目录行
\* （thread_store.rs:2100-2103 调用 insert_thread_metadata :1954-1981，
\* :1976 重名检查、:1979 提交；失败由 :2105-2106 的补偿删除刚写的文件）。
CreateCatalog ==
    /\ pc = "file"
    \* thread_store.rs:1976：目录行或删除标记存在则拒绝
    /\ catalog = "absent"
    /\ catalog' = "present"
    /\ pc' = "done"
    /\ UNCHANGED <<file, orphan, appended, rejected>>

\* 进程在文件写入与目录发布之间崩溃：重试 create 生成新的时间戳文件名
\* （rollout.rs:251-267），旧带头文件成为谱系无引用的孤儿；
\* 目录行从未发布，所有读路径经 stored()（thread_store.rs:1335-1345）不可见。
Crash ==
    /\ pc = "file"
    /\ orphan' = TRUE
    /\ file' = "absent"
    /\ pc' = "idle"
    /\ UNCHANGED <<catalog, appended, rejected>>

\* 修复后 append_records 先校验当前 rollout 的 plain/compressed 表示存在
\* （thread_store.rs:2245-2251），缺失即 CorruptHistory 拒绝（:2252-2256，
\* 检测旧坏状态），不再经 ensure_open(create=true) 惰性创建无头文件。
\* 正常线程目录行发布前头已写好，永远走 file="head" 分支。
Append ==
    \* create saga 全程持有线程 gate 与 owner 锁（thread_store.rs:2068-2070）
    /\ catalog = "present"
    \* append 与 create 经同一线程 gate 互斥（thread_store.rs:2229-2230）
    /\ pc \in {"idle", "done"}
    /\ appended' = TRUE
    /\ IF file = "head"
       THEN UNCHANGED <<file, rejected>>
       ELSE /\ rejected' = TRUE
            /\ UNCHANGED file
    /\ UNCHANGED <<catalog, orphan, pc>>

Next == CreateHead \/ CreateCatalog \/ Crash \/ Append

TypeOK ==
    /\ catalog \in {"absent", "present"}
    /\ file \in {"absent", "head", "headless"}
    /\ orphan \in BOOLEAN
    /\ pc \in {"idle", "file", "done"}
    /\ appended \in BOOLEAN
    /\ rejected \in BOOLEAN

\* 规则1：静止状态下目录有行的线程其当前 rollout 必须已含 SessionMeta 头。
\* 变更点：create_thread（thread_store.rs:2078-2110）与 fork_known_source
\* （:1907-1937）反转为先写头、后发布目录行。
NoMetadataWithoutHead ==
    (pc = "idle" /\ catalog = "present") => file = "head"

\* 规则2：任何 rollout 文件的首条记录必须是 SessionMeta，不允许无头历史。
\* 变更点：append_records 的目录行 ⇒ 文件存在防御（thread_store.rs:2238-2256）。
NoHeadlessRollout == file # "headless"

Spec == Init /\ [][Next]_vars
=============================================================================
