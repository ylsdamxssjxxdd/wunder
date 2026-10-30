--------------------------- MODULE GoalRoundDriverFixed ---------------------------
\* 修复后的模型（当前代码）：驱动在可用性探测之前先记下 deferred；会话栅栏释放时
\* 由租约唤醒重新请求驱动，被挡回的唤醒不再丢失。
EXTENDS Naturals
CONSTANT Cap
VARIABLES goal, armed, busy, rounds, inflight, requested, deferred
vars == <<goal, armed, busy, rounds, inflight, requested, deferred>>

Init ==
    /\ goal = "none"
    /\ armed = FALSE
    /\ busy = TRUE               \* crates/wunder-desktop/native_stream.rs:238
    /\ rounds = 0
    /\ inflight = FALSE
    /\ requested = FALSE
    /\ deferred = FALSE

CreateGoalDuringTurn ==
    /\ goal = "none"             \* crates/wunder-runtime/src/services/goal/service.rs:215
    /\ busy                       \* crates/wunder-desktop/native_stream.rs:238
    /\ goal' = "active"
    /\ armed' = TRUE
    /\ requested' = TRUE         \* crates/wunder-runtime/src/services/goal/driver.rs:169
    /\ UNCHANGED <<busy, rounds, inflight, deferred>>

DriveWhileBusy ==
    \* 探测前先置 deferred，再发现会话忙碌；唤醒被记下而不是丢弃。
    /\ requested = TRUE          \* crates/wunder-runtime/src/services/goal/driver.rs:169
    /\ busy                       \* crates/wunder-runtime/src/services/goal/driver.rs:330
    /\ requested' = FALSE
    /\ deferred' = TRUE          \* crates/wunder-runtime/src/services/goal/driver.rs:329
    /\ UNCHANGED <<goal, armed, busy, rounds, inflight>>

DriveWhenIdle ==
    /\ requested = TRUE          \* crates/wunder-runtime/src/services/goal/driver.rs:169
    /\ ~busy                      \* crates/wunder-runtime/src/services/goal/driver.rs:330
    /\ ~inflight                  \* crates/wunder-runtime/src/services/goal/driver.rs:317
    /\ rounds < Cap
    /\ rounds' = rounds + 1       \* crates/wunder-runtime/src/services/runtime/thread/runtime.rs:410
    /\ inflight' = TRUE
    /\ requested' = FALSE
    /\ deferred' = FALSE         \* crates/wunder-runtime/src/services/goal/driver.rs:333
    /\ UNCHANGED <<goal, armed, busy>>

RoundEnds ==
    /\ inflight = TRUE            \* crates/wunder-runtime/src/orchestrator/execute.rs:2271
    /\ inflight' = FALSE
    /\ requested' = TRUE
    /\ UNCHANGED <<goal, armed, busy, rounds, deferred>>

LeaseRelease ==
    \* 租约释放：只有先记下过被挡回的唤醒，才重新请求驱动。
    /\ busy                       \* crates/wunder-desktop/native_stream.rs:238
    /\ busy' = FALSE
    /\ requested' = IF deferred THEN TRUE ELSE requested  \* crates/wunder-runtime/src/services/goal/driver.rs:200; crates/wunder-runtime/src/services/runtime/thread/runtime.rs:189
    /\ deferred' = FALSE
    /\ UNCHANGED <<goal, armed, rounds, inflight>>

Next == CreateGoalDuringTurn \/ DriveWhileBusy \/ DriveWhenIdle \/ RoundEnds \/ LeaseRelease
Spec == Init /\ [][Next]_vars

System == DriveWhileBusy \/ DriveWhenIdle \/ RoundEnds \/ LeaseRelease
Settled == ~ENABLED System

Stalled ==
    /\ goal = "active"
    /\ armed
    /\ ~busy
    /\ ~inflight
    /\ rounds < Cap

\* 目标一旦活跃且武装，只要会话空闲且未达轮次上限，就必须继续开轮。
\* crates/wunder-desktop/native_stream.rs:238; crates/wunder-runtime/src/services/goal/driver.rs:200
GoalKeepsDriving == Settled => ~Stalled

\* 驱动不会超出目标轮次上限。
CapRespected == rounds <= Cap

\* 人类回合持有栅栏期间不会有目标轮在运行。
RoundsOnlyWhenIdle == inflight => ~busy
=============================================================================
