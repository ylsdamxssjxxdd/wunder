--------------------------- MODULE CronLifecycleFixed ---------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANT Fixed
VARIABLES row, pc, snapshot, active, expiredWorkers, expiredOnce,
          disabled, edited, deleted, removed, duplicate, staleWrite, partialHistory,
          history, committed
vars == <<row, pc, snapshot, active, expiredWorkers, expiredOnce,
          disabled, edited, deleted, removed, duplicate, staleWrite, partialHistory,
          history, committed>>
Workers == 1..2
Empty == [present |-> FALSE, enabled |-> FALSE, lease |-> 0, revision |-> 0]
Init == /\ row = [present |-> TRUE, enabled |-> TRUE, lease |-> 0, revision |-> 0]
        /\ pc = [worker \in Workers |-> 0]
        /\ snapshot = [worker \in Workers |-> Empty]
        /\ active = {} /\ expiredWorkers = {} /\ expiredOnce = FALSE
        /\ disabled = FALSE /\ edited = FALSE /\ deleted = FALSE /\ removed = FALSE
        /\ duplicate = FALSE /\ staleWrite = FALSE /\ partialHistory = FALSE
        /\ history = {} /\ committed = {}
Available == row.present /\ (row.lease = 0 \/ row.lease \in expiredWorkers)
ReadManual(worker) ==
    /\ pc[worker] = 0 /\ Available \* crates/wunder-runtime/src/services/cron.rs:521,534
    /\ snapshot' = [snapshot EXCEPT ![worker] = row]
    /\ pc' = [pc EXCEPT ![worker] = 1]
    /\ UNCHANGED <<row, active, expiredWorkers, expiredOnce, disabled, edited, deleted,
                    removed, duplicate, staleWrite, partialHistory, history, committed>>
ClaimManual(worker) ==
    /\ pc[worker] = 1 \* crates/wunder-runtime/src/services/cron.rs:546; crates/wunder-runtime/src/storage/sqlite/cron.rs:100
    /\ IF ~Fixed \/ row = snapshot[worker]
          THEN /\ row' = [snapshot[worker] EXCEPT !.lease = worker]
               /\ active' = active \cup {worker}
               /\ duplicate' = (duplicate \/ (active \ expiredWorkers # {}))
               /\ pc' = [pc EXCEPT ![worker] = 2]
          ELSE /\ pc' = [pc EXCEPT ![worker] = 5]
               /\ UNCHANGED <<row, active, duplicate>>
    /\ UNCHANGED <<snapshot, expiredWorkers, expiredOnce, disabled, edited, deleted,
                    removed, staleWrite, partialHistory, history, committed>>
ClaimTimer(worker) ==
    /\ pc[worker] = 0 /\ Available /\ row.enabled \* crates/wunder-runtime/src/storage/sqlite/cron.rs:294; crates/wunder-runtime/src/storage/postgres/cron.rs:286
    /\ row' = [row EXCEPT !.lease = worker]
    /\ active' = active \cup {worker}
    /\ duplicate' = (duplicate \/ (active \ expiredWorkers # {}))
    /\ pc' = [pc EXCEPT ![worker] = 2]
    /\ UNCHANGED <<snapshot, expiredWorkers, expiredOnce, disabled, edited, deleted,
                    removed, staleWrite, partialHistory, history, committed>>
ReadFinish(worker) ==
    /\ pc[worker] = 2 \* crates/wunder-runtime/src/services/cron.rs:2295
    /\ IF row.present /\ row.lease = worker
          THEN /\ snapshot' = [snapshot EXCEPT ![worker] = row]
               /\ pc' = [pc EXCEPT ![worker] = 3]
               /\ UNCHANGED active
          ELSE /\ pc' = [pc EXCEPT ![worker] = 5]
               /\ active' = active \ {worker}
               /\ UNCHANGED snapshot
    /\ UNCHANGED <<row, expiredWorkers, expiredOnce, disabled, edited, deleted,
                    removed, duplicate, staleWrite, partialHistory, history, committed>>
WriteHistory(worker) ==
    /\ ~Fixed /\ pc[worker] = 3 \* pre-fix cron.rs:2295 separate insert_cron_run
    /\ history' = history \cup {worker}
    /\ partialHistory' = (partialHistory \/ ~row.present \/ row.lease # worker)
    /\ pc' = [pc EXCEPT ![worker] = 4]
    /\ UNCHANGED <<row, snapshot, active, expiredWorkers, expiredOnce, disabled, edited,
                    deleted, removed, duplicate, staleWrite, committed>>
WriteFinish(worker) ==
    /\ (Fixed /\ pc[worker] = 3) \/ (~Fixed /\ pc[worker] = 4) \* cron.rs:2385
    /\ IF ~Fixed \/ row = snapshot[worker]
          THEN /\ row' = [snapshot[worker] EXCEPT !.lease = 0]
               /\ committed' = committed \cup {worker}
               /\ history' = history \cup {worker}
               /\ staleWrite' = (staleWrite \/ (row # snapshot[worker]))
               /\ pc' = [pc EXCEPT ![worker] = 5]
               /\ active' = active \ {worker}
          ELSE /\ pc' = [pc EXCEPT ![worker] = 2]
               /\ UNCHANGED <<row, committed, history, staleWrite, active>>
    /\ UNCHANGED <<snapshot, expiredWorkers, expiredOnce, disabled, edited, deleted,
                    removed, duplicate, partialHistory>>
Disable ==
    /\ ~disabled /\ row.present \* crates/wunder-runtime/src/services/cron.rs:440
    /\ row' = [row EXCEPT !.enabled = FALSE, !.lease = IF Fixed THEN @ ELSE 0,
                          !.revision = @ + 1]
    /\ disabled' = TRUE
    /\ UNCHANGED <<pc, snapshot, active, expiredWorkers, expiredOnce, edited, deleted,
                    removed, duplicate, staleWrite, partialHistory, history, committed>>
Edit ==
    /\ ~edited /\ row.present \* crates/wunder-runtime/src/services/cron.rs:406
    /\ row' = [row EXCEPT !.revision = @ + 1]
    /\ edited' = TRUE
    /\ UNCHANGED <<pc, snapshot, active, expiredWorkers, expiredOnce, disabled, deleted,
                    removed, duplicate, staleWrite, partialHistory, history, committed>>
Delete ==
    /\ ~deleted /\ row.present \* crates/wunder-runtime/src/services/cron.rs:498
    /\ row' = [row EXCEPT !.present = FALSE]
    /\ deleted' = TRUE /\ removed' = TRUE
    /\ UNCHANGED <<pc, snapshot, active, expiredWorkers, expiredOnce, disabled, edited,
                    duplicate, staleWrite, partialHistory, history, committed>>
Expire ==
    /\ ~expiredOnce /\ row.present /\ row.lease # 0 \* cron.rs:1129 lease_expires_at comparison
    /\ expiredWorkers' = expiredWorkers \cup {row.lease}
    /\ expiredOnce' = TRUE
    /\ UNCHANGED <<row, pc, snapshot, active, disabled, edited, deleted, removed,
                    duplicate, staleWrite, partialHistory, history, committed>>
System == (\E worker \in Workers: ReadManual(worker) \/ ClaimManual(worker) \/
           ClaimTimer(worker) \/ ReadFinish(worker) \/ WriteHistory(worker) \/ WriteFinish(worker)) \/ Expire
Next == System \/ Disable \/ Edit \/ Delete
Spec == Init /\ [][Next]_vars
Settled == ~ENABLED System
\* 删除完成后，迟到的领取与完成回写不得复活任务。cron.rs:557; crates/wunder-runtime/src/services/cron.rs:2385
NoResurrection == removed => ~row.present
\* 租约未失效时同一任务不得重复获得执行权。cron.rs:557; sqlite/cron.rs:294
ExclusiveClaim == ~duplicate
\* 完成回写不得覆盖并发编辑、停用或新的租约。cron.rs:2385
NoStaleCompletion == ~staleWrite
\* 运行历史不得在删除或换代后单独写入。sqlite/cron.rs:100; crates/wunder-runtime/src/storage/postgres/cron.rs:96
AtomicHistory == ~partialHistory
\* 修复后运行历史与状态提交必须处于同一事务。sqlite/cron.rs:146
HistoryWithCommit == Fixed => history = committed
=============================================================================
