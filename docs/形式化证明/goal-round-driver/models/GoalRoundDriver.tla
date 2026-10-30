--------------------------- MODULE GoalRoundDriver ---------------------------
\* 修复前（现场复现）的模型：人类回合持有会话栅栏时创建/变更目标，驱动唤醒被
\* 忙碌栅栏挡回；回合结束释放栅栏后没有任何目标唤醒，活跃目标从此不再开轮。
\* 对照模型：GoalRoundDriverFixed.tla（修复后的当前代码）。
EXTENDS Naturals
CONSTANT Cap
VARIABLES goal, armed, busy, rounds, inflight, requested
vars == <<goal, armed, busy, rounds, inflight, requested>>

Init ==
    \* 人类回合正在运行，并持有会话栅栏（Direct-Run 租约直到流泵结束）。
    /\ goal = "none"
    /\ armed = FALSE
    /\ busy = TRUE               \* crates/wunder-desktop/native_stream.rs:238
    /\ rounds = 0
    /\ inflight = FALSE
    /\ requested = FALSE

CreateGoalDuringTurn ==
    \* 模型在回合中通过 goal 工具创建目标，领域服务唤醒驱动。
    /\ goal = "none"             \* crates/wunder-runtime/src/services/goal/service.rs:215
    /\ busy                       \* crates/wunder-desktop/native_stream.rs:238
    /\ goal' = "active"
    /\ armed' = TRUE
    /\ requested' = TRUE         \* crates/wunder-runtime/src/services/goal/driver.rs:169
    /\ UNCHANGED <<busy, rounds, inflight>>

DriveWhileBusy ==
    \* 驱动任务开始执行，可用性探测发现会话被人类回合占用，直接放弃。
    /\ requested = TRUE          \* crates/wunder-runtime/src/services/goal/driver.rs:169
    /\ busy                       \* crates/wunder-runtime/src/services/goal/driver.rs:330
    /\ requested' = FALSE
    /\ UNCHANGED <<goal, armed, busy, rounds, inflight>>

DriveWhenIdle ==
    \* 会话空闲且未在轮中、未达轮次上限时提交下一轮目标回合。
    /\ requested = TRUE          \* crates/wunder-runtime/src/services/goal/driver.rs:169
    /\ ~busy                      \* crates/wunder-runtime/src/services/goal/driver.rs:330
    /\ ~inflight                  \* crates/wunder-runtime/src/services/goal/driver.rs:317
    /\ rounds < Cap
    /\ rounds' = rounds + 1       \* crates/wunder-runtime/src/services/runtime/thread/runtime.rs:410
    /\ inflight' = TRUE
    /\ requested' = FALSE
    /\ UNCHANGED <<goal, armed, busy>>

RoundEnds ==
    \* 目标轮次结束，轮次结束唤醒重新请求驱动。
    /\ inflight = TRUE            \* crates/wunder-runtime/src/orchestrator/execute.rs:2271
    /\ inflight' = FALSE
    /\ requested' = TRUE
    /\ UNCHANGED <<goal, armed, busy, rounds>>

LeaseRelease ==
    \* 回合结束，流泵完成后释放 Direct-Run 租约；修复前这里没有任何目标唤醒。
    /\ busy                       \* crates/wunder-desktop/native_stream.rs:238
    /\ busy' = FALSE
    /\ UNCHANGED <<goal, armed, rounds, inflight, requested>>

Next == CreateGoalDuringTurn \/ DriveWhileBusy \/ DriveWhenIdle \/ RoundEnds \/ LeaseRelease
Spec == Init /\ [][Next]_vars

\* 系统自身的推进（不含人写目标这一步），用于判断“已经停下来”。
System == DriveWhileBusy \/ DriveWhenIdle \/ RoundEnds \/ LeaseRelease
Settled == ~ENABLED System

\* 活跃且武装的目标在空闲会话、未在轮中、未达上限时，必须还能再开一轮。
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
