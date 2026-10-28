--------------------------- MODULE ThreadOwnership ---------------------------
\* FV-01 旧行为模型：线程打开权、执行权与过期持有者隔离。
\* 基线提交 508e12a。两个本地实例（Executors）竞争同一任务线程。
\* 打开租约（thread_open_leases）与执行锁（session_locks）是两个独立事实：
\* 租约有 fencing token，执行锁没有。时间用 alive 布尔抽象 TTL（时延另测）。
EXTENDS Integers
CONSTANTS Executors, MaxGen, MaxTok
VARIABLES lockRow, leaseRow, nextGen, nextTok, pc, myTok, myGen, foreignRelease, foreignTouch
vars == <<lockRow, leaseRow, nextGen, nextTok, pc, myTok, myGen, foreignRelease, foreignTouch>>

NoOwner == "none"
EmptyLock == [owner |-> NoOwner, gen |-> 0, alive |-> FALSE]
EmptyLease == [owner |-> NoOwner, tok |-> 0, alive |-> FALSE]

Init ==
    /\ lockRow = EmptyLock
    /\ leaseRow = EmptyLease
    /\ nextGen = 0
    /\ nextTok = 0
    /\ pc = [e \in Executors |-> "idle"]
    /\ myTok = [e \in Executors |-> 0]
    /\ myGen = [e \in Executors |-> 0]
    /\ foreignRelease = FALSE
    /\ foreignTouch = FALSE

\* 打开线程。crates/wunder-runtime/src/storage/sqlite/thread_open_lease_store.rs:53-125
\* 单个 Immediate 事务：先删过期行(:72-74)，同持有者刷新保代(:87-92)，
\* 他人持有则 Busy 不建模；无行时活跃执行锁阻止打开(:99-110)，否则代次+1 插入(:112-125)。
AcquireLease(e) ==
    /\ pc[e] = "idle"
    /\ nextTok < MaxTok
    /\ IF leaseRow.owner # NoOwner /\ leaseRow.alive
       THEN /\ leaseRow.owner = e
            /\ myTok' = [myTok EXCEPT ![e] = leaseRow.tok]
            /\ pc' = [pc EXCEPT ![e] = "open"]
            /\ UNCHANGED <<lockRow, leaseRow, nextGen, nextTok, myGen, foreignRelease, foreignTouch>>
       ELSE /\ ~(lockRow.owner # NoOwner /\ lockRow.alive)
            /\ nextTok' = nextTok + 1
            /\ leaseRow' = [owner |-> e, tok |-> nextTok', alive |-> TRUE]
            /\ myTok' = [myTok EXCEPT ![e] = nextTok']
            /\ pc' = [pc EXCEPT ![e] = "open"]
            /\ UNCHANGED <<lockRow, nextGen, myGen, foreignRelease, foreignTouch>>

\* 获得执行权。crates/wunder-runtime/src/storage/sqlite/session_lock_store.rs:43-105
\* 事务内先删过期行(:55-57)；活跃行冲突返回 UserBusy(:88-93)，不建模该自旋。
AcquireLock(e) ==
    /\ pc[e] = "open"
    /\ nextGen < MaxGen
    /\ ~(lockRow.owner # NoOwner /\ lockRow.alive)
    /\ nextGen' = nextGen + 1
    /\ lockRow' = [owner |-> e, gen |-> nextGen', alive |-> TRUE]
    /\ myGen' = [myGen EXCEPT ![e] = nextGen']
    /\ pc' = [pc EXCEPT ![e] = "exec"]
    /\ UNCHANGED <<leaseRow, nextTok, myTok, foreignRelease, foreignTouch>>

\* TTL 到期。行在下一次 acquire 事务中才被物理删除，这里直接失去活性；
\* 持有者不知情，pc 保持 exec（物理执行残留，恢复后才会发现自己失权）。
ExpireLock ==
    /\ lockRow.owner # NoOwner
    /\ lockRow.alive
    /\ lockRow' = [lockRow EXCEPT !.alive = FALSE]
    /\ UNCHANGED <<leaseRow, nextGen, nextTok, pc, myTok, myGen, foreignRelease, foreignTouch>>

ExpireLease ==
    /\ leaseRow.owner # NoOwner
    /\ leaseRow.alive
    /\ leaseRow' = [leaseRow EXCEPT !.alive = FALSE]
    /\ UNCHANGED <<lockRow, nextGen, nextTok, pc, myTok, myGen, foreignRelease, foreignTouch>>

\* 执行中心跳续期。旧代码：session_lock_store.rs:117-120，UPDATE 只按 session_id 匹配，
\* 不检查持有者身份，也不检查行是否已过期；调用点 execute.rs:257-272。
HeartbeatLock(e) ==
    \* execute.rs:257-272：心跳任务随已获得的执行启动，持续到 finish_request_resources
    /\ pc[e] = "exec"
    /\ lockRow.owner # NoOwner
    /\ lockRow' = [lockRow EXCEPT !.alive = TRUE]
    /\ IF lockRow.owner # e \/ lockRow.gen # myGen[e]
       THEN foreignTouch' = TRUE
       ELSE UNCHANGED foreignTouch
    /\ UNCHANGED <<leaseRow, nextGen, nextTok, pc, myTok, myGen, foreignRelease>>

\* 执行结束释放锁。旧代码：session_lock_store.rs:130-134，DELETE 只按 session_id 匹配；
\* 调用点 execute_tools.rs:33-36（finish_request_resources 对 acquired 的执行无条件 release）。
ReleaseLock(e) ==
    /\ pc[e] = "exec"
    /\ IF lockRow.owner # NoOwner /\ (lockRow.owner # e \/ lockRow.gen # myGen[e])
       THEN foreignRelease' = TRUE
       ELSE UNCHANGED foreignRelease
    /\ lockRow' = EmptyLock
    /\ pc' = [pc EXCEPT ![e] = "open"]
    /\ UNCHANGED <<leaseRow, nextGen, nextTok, myTok, myGen, foreignTouch>>

\* 关闭线程释放打开租约。thread_open_lease_store.rs:151，DELETE 带 owner+token 条件（已 fenced）。
ReleaseLease(e) ==
    /\ pc[e] = "open"
    /\ myTok[e] # 0
    /\ IF leaseRow.owner = e /\ leaseRow.tok = myTok[e]
       THEN leaseRow' = EmptyLease
       ELSE UNCHANGED leaseRow
    /\ myTok' = [myTok EXCEPT ![e] = 0]
    /\ pc' = [pc EXCEPT ![e] = "idle"]
    /\ UNCHANGED <<lockRow, nextGen, nextTok, myGen, foreignRelease, foreignTouch>>

Next ==
    \/ \E e \in Executors: AcquireLease(e) \/ AcquireLock(e) \/ HeartbeatLock(e) \/ ReleaseLock(e) \/ ReleaseLease(e)
    \/ ExpireLock \/ ExpireLease

TypeOK ==
    /\ lockRow.owner \in Executors \cup {NoOwner}
    /\ lockRow.gen \in 0..MaxGen
    /\ lockRow.alive \in BOOLEAN
    /\ leaseRow.owner \in Executors \cup {NoOwner}
    /\ leaseRow.tok \in 0..MaxTok
    /\ leaseRow.alive \in BOOLEAN
    /\ pc \in [Executors -> {"idle", "open", "exec"}]
    /\ \A e \in Executors: myTok[e] \in 0..MaxTok /\ myGen[e] \in 0..MaxGen

\* 规则1：过期失权旧持有者的迟到释放，绝不能删除当前属于他人代次的执行锁行。
\* 变更点：session_lock_store.rs:130-134。
LateReleaseKeepsLock == ~foreignRelease

\* 规则2：过期失权旧持有者的迟到心跳，绝不能给他人代次的执行锁行续期。
\* 变更点：session_lock_store.rs:117-120。
LateTouchKeepsLock == ~foreignTouch

Spec == Init /\ [][Next]_vars
=============================================================================
