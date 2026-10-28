--------------------------- MODULE StopCancelFixed ---------------------------
\* FV-04 修复模型：停止、取消、超时与执行结清（修复后语义）。
\* 与 StopCancel.tla（旧模型）的差异：
\*   1. 执行体收尾拆为 检查(FinishCheck) → 提交(FinishCommit/FinishCommitRejected) 两段，
\*      进入收尾先过取消闸，持久层拒绝完成写入即改按取消语义结清；
\*   2. 取消方尾部补写监视器时尊重终态（finished/error 不改写）。
\* 冻结契约的四阶段：停止请求(RequestStop) → 取消已受理(FenceAndMonitor+CancelTurns)
\* → 执行已隔离(CheckpointSeesCancel/FinishSeesCancel/FinishCommit 系列/HitTimeout 之一结清执行体)
\* → 外部命令结束(KillCommands，执行体自行结清时也带走命令)。
\* turn 是持久控制面（admissions+canonical，写入有守卫）；monitor/frame/goal 是呈现面。
EXTENDS Naturals

VARIABLES turn, monitor, executor, stopper, command, frame, goal, cancelVisible, reopened
vars == <<turn, monitor, executor, stopper, command, frame, goal, cancelVisible, reopened>>

TurnStates == {"run", "done", "stop", "failed"}
MonitorStates == {"run", "halt", "done", "stop", "error"}
ExecutorStates == {"stream", "finish", "commit", "off"}
\* "over"：本执行生命周期的停止管线已随重开关闭（Reopen 后置位），
\* 用它而不是全局 ~reopened 来限界——模型只刻画一个执行生命周期。
StopperStates == {"idle", "asked", "fenced", "cut", "killed", "over"}
CommandStates == {"none", "run", "dead"}
FrameStates == {"none", "done", "stop", "failed"}
GoalStates == {"none", "done", "stop", "failed"}

Init ==
    /\ turn = "run"
    /\ monitor = "run"
    /\ executor = "stream"
    /\ stopper = "idle"
    /\ command \in {"none", "run"}
    /\ frame = "none"
    /\ goal = "none"
    /\ cancelVisible = FALSE
    /\ reopened = FALSE

\* ---- 停止管线（读者动作之外均为系统动作） ----

\* 停止请求。入口：REST crates/wunder-runtime/src/api/chat.rs:762-767、
\* WS crates/wunder-runtime/src/api/chat_ws.rs:1059-1077、桌面 crates/wunder-desktop/native_stream.rs:292-296。
\* 入口先清 goal 再进取消管线，goal 清空本身不建模（取消语义由停止管线保证）。
RequestStop ==
    /\ stopper = "idle"
    /\ stopper' = "asked"
    /\ UNCHANGED <<turn, monitor, executor, command, frame, goal, cancelVisible, reopened>>

\* 受理：执行围栏 + 监视器置 cancelling。此后执行体的取消检查点必然可见。
\* 围栏先于监视器置位：runtime.rs:706-723（execution.interrupted=true）；runtime.rs:724（cancel_with_source）。
FenceAndMonitor ==
    /\ stopper = "asked"
    /\ cancelVisible' = TRUE
    \* monitor.rs:1345-1351：仅活跃态（running/cancelling/queued/waiting）受理；终态记录提前返回
    /\ monitor' = IF monitor = "run" THEN "halt" ELSE monitor
    /\ stopper' = "fenced"
    /\ UNCHANGED <<turn, executor, command, frame, goal, reopened>>

\* 取消已受理的持久写入：活跃轮次写 cancelled。守卫：终态不可逆。
\* runtime.rs:832-836（cancel_active_turns）；thread_admission.rs:622-635（can_transition）。
CancelTurns ==
    /\ stopper = "fenced"
    \* thread_admission.rs:622-635：仅非终态可迁移，cancelled/completed/failed 轮次保持不变
    /\ turn' = IF turn = "run" THEN "stop" ELSE turn
    /\ stopper' = "cut"
    /\ UNCHANGED <<monitor, executor, command, frame, goal, cancelVisible, reopened>>

\* 外部命令结束：进程 token 熔断 → 命令循环 kill 子进程；超时 kill 走同一路径。
\* runtime.rs:727-729（terminate_scope）；broker.rs:610-637；command_tool.rs:853-862。
\* 修复后尾部补写：执行体已不在且监视器不在终态时才落 cancelled。
KillCommands ==
    /\ stopper = "cut"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    \* runtime.rs:858-863：执行体已不在时取消方补写终态；
    \* monitor.rs:1295-1301：finished/error 终态记录不再改写为 cancelled
    /\ monitor' = IF executor = "off" /\ monitor \in {"run", "halt", "stop"} THEN "stop" ELSE monitor
    /\ stopper' = "killed"
    /\ UNCHANGED <<turn, executor, frame, goal, cancelVisible, reopened>>

\* ---- 执行体 ----

\* 执行中观察到取消：检查点短路，走 CANCELLED 结清（终态帧/监视器/goal 均为取消语义）。
\* 观察点：llm.rs:330-343（ensure_not_cancelled）、llm.rs:345-394（wait_for_cancelled）。
\* 结清：execute_tools.rs:116-140（update_turn cancelled，守卫幂等）、:166-178（mark_cancelled）。
CheckpointSeesCancel ==
    /\ executor = "stream"
    \* llm.rs:330-343：monitor.is_cancelled 或 execution_interrupted 为真才走取消结清
    /\ cancelVisible
    /\ executor' = "off"
    /\ turn' = "stop"
    /\ monitor' = "stop"
    /\ frame' = "stop"
    /\ goal' = "stop"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    /\ UNCHANGED <<stopper, cancelVisible, reopened>>

\* 通过最后一个取消检查点，进入收尾窗口。
\* execute.rs:2118-2148（break 之后只有答案装配与用量落账，无 ensure_not_cancelled）。
PassLastCheckpoint ==
    /\ executor = "stream"
    /\ executor' = "finish"
    /\ UNCHANGED <<turn, monitor, stopper, command, frame, goal, cancelVisible, reopened>>

\* 修复点：收尾前置取消闸。已受理的停止在这里直接按取消语义结清，不再进入完成提交。
\* execute_tools.rs:205-207（finish_request_success 顶部 ensure_not_cancelled，
\* 同构修复：memory.rs:1791-1796 手动压缩结清前置闸）。
FinishSeesCancel ==
    /\ executor = "finish"
    \* execute_tools.rs:207：ensure_not_cancelled 命中即返回 CANCELLED，走 finish_request_error 取消结清
    /\ cancelVisible
    /\ executor' = "off"
    /\ turn' = IF turn = "run" THEN "stop" ELSE turn
    /\ monitor' = "stop"
    /\ frame' = "stop"
    /\ goal' = "stop"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    /\ UNCHANGED <<stopper, cancelVisible, reopened>>

\* 前置闸通过，进入完成提交窗口（提交前停止仍可能受理，由提交结果裁决）。
FinishCheck ==
    /\ executor = "finish"
    \* execute_tools.rs:207：取消不可见才允许进入完成提交
    /\ ~cancelVisible
    /\ executor' = "commit"
    /\ UNCHANGED <<turn, monitor, stopper, command, frame, goal, cancelVisible, reopened>>

\* 完成提交被持久层采纳：发出完成语义（终态帧/监视器/目标结局/记忆）。
\* execute_tools.rs:221-241（采纳 update_turn 返回值，Ok(true) 才继续完成语义）。
FinishCommit ==
    /\ executor = "commit"
    \* thread_log_committer.rs:221-226 + thread_admission.rs:622-635：仅 running→completed 被采纳
    /\ turn = "run"
    /\ turn' = "done"
    /\ frame' = "done"
    /\ monitor' = "done"
    /\ goal' = "done"
    /\ executor' = "off"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    /\ UNCHANGED <<stopper, cancelVisible, reopened>>

\* 完成提交被持久层拒绝（轮次已取消）：改按取消语义结清，不发完成帧、不改监视器为完成。
\* execute_tools.rs:235-241（!accepted 返回 CANCELLED，走 finish_request_error 取消结清；
\* 同构修复：memory.rs:1816-1841 手动压缩 Ok(false) 分支）。
FinishCommitRejected ==
    /\ executor = "commit"
    \* execute_tools.rs:237-241：update_turn 返回 Ok(false) 即按取消结清
    /\ turn /= "run"
    /\ executor' = "off"
    /\ frame' = "stop"
    /\ monitor' = "stop"
    /\ goal' = "stop"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    /\ UNCHANGED <<turn, stopper, cancelVisible, reopened>>

\* 超时结清：模型流超时/任务 TTL 走 failed 语义（finish_request_error 非 CANCELLED 分支）。
\* command_tool.rs:857（命令超时 kill）；execute_tools.rs:179-181（mark_error）。
HitTimeout ==
    /\ executor = "stream"
    \* 取消已可见时取消语义优先：检查点先行短路（llm.rs:330-343），
    \* 且 finish_request_error 的 CANCELLED 分支先于失败结清（execute_tools.rs:166-178）
    /\ ~cancelVisible
    /\ executor' = "off"
    /\ turn' = "failed"
    /\ monitor' = "error"
    /\ frame' = "failed"
    /\ goal' = "failed"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    /\ UNCHANGED <<stopper, cancelVisible, reopened>>

\* 结清后重开新轮次：register 重置取消标志，新租约新 token，旧 token 保持熔断。
\* monitor.rs:2562-2563（register_locked 重置 cancel_requested/cancel_source）；
\* runtime.rs:951-959（新 LeaseExecution interrupted=false）；child_runs.rs:58-59（新 token）、:46-48（旧执行未结清拒绝重开）。
Reopen ==
    /\ ~reopened
    \* child_runs.rs:46-48：旧执行未结清（租约仍活跃）拒绝重开
    /\ executor = "off"
    /\ turn \in {"done", "stop", "failed"}
    /\ reopened' = TRUE
    /\ turn' = "run"
    /\ monitor' = "run"
    /\ frame' = "none"
    /\ goal' = "none"
    /\ cancelVisible' = FALSE
    \* 本执行生命周期的停止管线关闭；模型只刻画一个生命周期，新轮次的停止不建模
    /\ stopper' = "over"
    /\ UNCHANGED <<executor, command>>

System ==
    \/ FenceAndMonitor
    \/ CancelTurns
    \/ KillCommands
    \/ CheckpointSeesCancel
    \/ PassLastCheckpoint
    \/ FinishSeesCancel
    \/ FinishCheck
    \/ FinishCommit
    \/ FinishCommitRejected
    \/ HitTimeout

Next == System \/ RequestStop \/ Reopen

\* 一切平息：只剩读者动作（停止请求/重开）可做。
Settled == ~ENABLED System

TypeOK ==
    /\ turn \in TurnStates
    /\ monitor \in MonitorStates
    /\ executor \in ExecutorStates
    /\ stopper \in StopperStates
    /\ command \in CommandStates
    /\ frame \in FrameStates
    /\ goal \in GoalStates
    /\ cancelVisible \in BOOLEAN
    /\ reopened \in BOOLEAN

\* 规则1：取消已受理（轮次终态为 cancelled）之后，任何呈现面都不得出现完成语义。
CancelNeverPresentedAsCompleted ==
    turn = "stop" => (frame /= "done" /\ monitor /= "done" /\ goal /= "done")

\* 规则2：平息后，轮次终态与监视器终态必须同语义（完成↔finished，取消↔cancelled，失败↔error）。
SettledViewsAgree ==
    (Settled /\ ~reopened) =>
        /\ (turn = "done") <=> (monitor = "done")
        /\ (turn = "stop") <=> (monitor = "stop")
        /\ (turn = "failed") <=> (monitor = "error")

\* 规则3：平息后没有仍在运行的外部命令。
SettledCommandsDead ==
    (Settled /\ ~reopened) => command /= "run"

\* 规则4：停止请求发出后，平息时停止管线必须走完四阶段。
\* "over" 是重开后本生命周期的管线关闭态，不属于"发出过停止请求"。
SettledStopCompletes ==
    (Settled /\ ~reopened /\ stopper /= "idle") => stopper = "killed"

\* 目标1（应为反例）：停止永远结清不出 cancelled —— 反例路径即"停止能结清"。
NeverSettlesCancelled == ~(Settled /\ turn = "stop")

\* 目标2（应为反例）：结清后永远不能再开新轮次 —— 反例路径即"停止后可再发言"。
NeverReopens == ~reopened

Spec == Init /\ [][Next]_vars
=============================================================================
