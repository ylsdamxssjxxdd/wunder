--------------------------- MODULE StopCancel ---------------------------
\* FV-04 旧行为模型：停止、取消、超时与执行结清。
\* 一个执行体（executor）、一条停止管线（stopper）、至多一个外部命令（command）。
\* 冻结契约的四阶段：停止请求(RequestStop) → 取消已受理(FenceAndMonitor+CancelTurns)
\* → 执行已隔离(CheckpointSeesCancel/FinishSuccess/HitTimeout 之一结清执行体)
\* → 外部命令结束(KillCommands，执行体自行结清时也带走命令)。
\* turn 是持久控制面（admissions+canonical，写入有守卫）；monitor/frame/goal 是呈现面。
EXTENDS Naturals

VARIABLES turn, monitor, executor, stopper, command, frame, goal, cancelVisible, reopened
vars == <<turn, monitor, executor, stopper, command, frame, goal, cancelVisible, reopened>>

TurnStates == {"run", "done", "stop", "failed"}
MonitorStates == {"run", "halt", "done", "stop", "error"}
ExecutorStates == {"stream", "finish", "off"}
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
    \* monitor.rs:1338-1344：仅活跃态（running/cancelling/queued/waiting）受理；终态记录提前返回
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
\* 旧行为尾部补写：执行体已不在时由取消方落 cancelled，但 mark_cancelled 无状态守卫——
\* runtime.rs:858-863（!has_active_executor && !holds_session_lock 才补写）；monitor.rs:1283-1323（无条件改写）。
KillCommands ==
    /\ stopper = "cut"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    \* runtime.rs:858-863：执行体已不在时取消方补写终态；monitor.rs:1283-1323 改写不查旧状态
    /\ monitor' = IF executor = "off" THEN "stop" ELSE monitor
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

\* 通过最后一个取消检查点，进入收尾窗口（此后到 finish_request_success 之间没有检查点）。
\* execute.rs:2118-2148（break 之后只有答案装配与用量落账，无 ensure_not_cancelled）。
PassLastCheckpoint ==
    /\ executor = "stream"
    /\ executor' = "finish"
    /\ UNCHANGED <<turn, monitor, stopper, command, frame, goal, cancelVisible, reopened>>

\* 收尾结清（finish_request_success）。持久写入有守卫，但写入结果被丢弃，完成语义照发。
\* 写入守卫：thread_log_committer.rs:221-226（can_transition 预检）；结果被丢弃：execute_tools.rs:218-230。
\* 完成语义：execute_tools.rs:232-244（turn_terminal{completed}）、:259-269（自动记忆）、
\* :270-274（mark_finished → monitor.rs:2675-2756 mark_status 无取消守卫）、execute.rs:2252-2270（goal Completed）。
FinishSuccess ==
    /\ executor = "finish"
    \* thread_log_committer.rs:221-226 + thread_admission.rs:622-635：仅 running→completed 被采纳
    /\ turn' = IF turn = "run" THEN "done" ELSE turn
    \* execute_tools.rs:218-230：旧行为不读 update_turn 的返回值，拒绝也走完成语义
    /\ frame' = "done"
    /\ monitor' = "done"
    /\ goal' = "done"
    /\ executor' = "off"
    /\ command' = IF command = "run" THEN "dead" ELSE command
    /\ UNCHANGED <<stopper, cancelVisible, reopened>>

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
\* monitor.rs:2555-2556（register_locked 重置 cancel_requested/cancel_source）；
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
    \/ FinishSuccess
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
\* 涉及：execute_tools.rs:185-276（finish_request_success）、execute.rs:2221/2265-2270。
CancelNeverPresentedAsCompleted ==
    turn = "stop" => (frame /= "done" /\ monitor /= "done" /\ goal /= "done")

\* 规则2：平息后，轮次终态与监视器终态必须同语义（完成↔finished，取消↔cancelled，失败↔error）。
\* 涉及：runtime.rs:1828-1832（is_task_cancelled 提前返回）与 :1835-1843/:2020-2021（fail_task 的 mark_cancelled）。
SettledViewsAgree ==
    (Settled /\ ~reopened) =>
        /\ (turn = "done") <=> (monitor = "done")
        /\ (turn = "stop") <=> (monitor = "stop")
        /\ (turn = "failed") <=> (monitor = "error")

\* 规则3：平息后没有仍在运行的外部命令。
\* 涉及：broker.rs:610-637（terminate_scope）、command_tool.rs:853-862（token 熔断 kill）。
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
