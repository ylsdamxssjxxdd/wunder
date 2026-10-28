------------------------ MODULE ThreadOwnershipFixed ------------------------
\* FV-01 修复模型：执行锁获得时颁发锁代次（lock_token），touch/release 携带并匹配。
\* 与旧模型的唯一差异：HeartbeatLock 与 ReleaseLock 增加持有者代次条件，
\* 对应 session_locks 增加 lock_token 列后 WHERE 条件带 session_id + lock_token。
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

\* 与旧模型相同：thread_open_lease_store.rs:53-125。
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

\* 与旧模型相同：session_lock_store.rs:25-110，但获得时同时拿到锁代次（:45 uuid）。
AcquireLock(e) ==
    /\ pc[e] = "open"
    /\ nextGen < MaxGen
    /\ ~(lockRow.owner # NoOwner /\ lockRow.alive)
    /\ nextGen' = nextGen + 1
    /\ lockRow' = [owner |-> e, gen |-> nextGen', alive |-> TRUE]
    /\ myGen' = [myGen EXCEPT ![e] = nextGen']
    /\ pc' = [pc EXCEPT ![e] = "exec"]
    /\ UNCHANGED <<leaseRow, nextTok, myTok, foreignRelease, foreignTouch>>

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

\* 修复：touch 的 WHERE 条件带 session_id + lock_token；他人代次的行不匹配，不动。
\* 自己的行即使已过期仍可续期（赶在接管前恢复心跳是合法竞态）。
HeartbeatLock(e) ==
    \* execute.rs:257-272：心跳任务随已获得的执行启动，持续到 finish_request_resources
    /\ pc[e] = "exec"
    \* 修复后 session_lock_store.rs:129 touch 的 WHERE：session_id + lock_token（行代次）
    /\ lockRow.owner = e
    \* session_lock_store.rs:129 的 WHERE 同时匹配 lock_token（模型中的行代次 gen）
    /\ lockRow.gen = myGen[e]
    /\ lockRow' = [lockRow EXCEPT !.alive = TRUE]
    /\ UNCHANGED <<leaseRow, nextGen, nextTok, pc, myTok, myGen, foreignRelease, foreignTouch>>

\* 修复：release 的 WHERE 条件带 session_id + lock_token；他人代次的行保留。
ReleaseLock(e) ==
    /\ pc[e] = "exec"
    /\ IF lockRow.owner = e /\ lockRow.gen = myGen[e]
       THEN lockRow' = EmptyLock
       ELSE UNCHANGED lockRow
    /\ pc' = [pc EXCEPT ![e] = "open"]
    /\ UNCHANGED <<leaseRow, nextGen, nextTok, myTok, myGen, foreignRelease, foreignTouch>>

\* 与旧模型相同：thread_open_lease_store.rs:151（已 fenced）。
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
\* 变更点：session_lock_store.rs:144 release 的 WHERE 增加 lock_token 匹配。
LateReleaseKeepsLock == ~foreignRelease

\* 规则2：过期失权旧持有者的迟到心跳，绝不能给他人代次的执行锁行续期。
\* 变更点：session_lock_store.rs:129 touch 的 WHERE 增加 lock_token 匹配。
LateTouchKeepsLock == ~foreignTouch

Spec == Init /\ [][Next]_vars
=============================================================================
