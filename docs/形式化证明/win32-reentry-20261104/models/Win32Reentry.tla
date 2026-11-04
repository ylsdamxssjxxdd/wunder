------------------------- MODULE Win32Reentry -------------------------
EXTENDS Naturals, Sequences

CONSTANTS MaxDepth, MaxExec, Fixed
VARIABLES frames, execPending, writeCtx
vars == <<frames, execPending, writeCtx>>

FrameKind == {"ime", "sys", "exec", "wedge"}
Frame == [kind: FrameKind, handled: BOOLEAN]
FrameSeq == UNION {[1..j -> Frame] : j \in 0..MaxDepth}
WedgeOnStack == \E i \in 1..Len(frames): frames[i].kind = "wedge"

(***************************************************************************)
(* 蛹 UI 线程的消息帧重入模型。winit 的窗口过程同步分发 Slint 事件          *)
(* （runner.rs:160 should_buffer 只看 event_handler 是否已被取走，           *)
(* 所以 Slint 的事件处理就运行在当前 WM_* 帧内），因此每个到达的消息占一个   *)
(* 帧，每帧恰好经历一次“Slint 处理”和一次“返回”。frames 是帧栈，栈底之外    *)
(* 的帧只可能来自同步重入。                                                  *)
(*                                                                          *)
(* 帧类别：ime = 输入法消息（WM_IME_*）；sys = 普通系统消息（鼠标/焦点/定时）；*)
(* exec = EXEC_MSG_ID 投递的延迟闭包（event_loop.rs:2838）；wedge = 线程已   *)
(* 卡死的吸收态，不再有任何动作。                                            *)
(*                                                                          *)
(* writeCtx 记录上一个原子步骤是否发生了“会回到输入法状态机的写调用”          *)
(* （ImmSetCompositionWindow / ImmSetCandidateWindow / ImmAssociateContextEx，*)
(* ime.rs:145 与 ime.rs:156）以及它所处的上下文。检查器在每个状态上验证写     *)
(* 调用的位置纪律；非写动作都把 writeCtx 复位为 "none"。                      *)
(***************************************************************************)

Init ==
    /\ frames = <<>>
    /\ execPending = 0
    /\ writeCtx = "none"

(************************ 泵轮：只在帧栈为空时收新消息 **********************)

ImeDeliversComposition ==
    \* event_loop.rs:1583 输入法组合消息到达窗口过程，压入 ime 帧
    /\ frames = <<>>
    /\ Len(frames) < MaxDepth              \* model bound; 调大 MaxDepth 只会拉长反例路径
    /\ frames' = <<[kind |-> "ime", handled |-> FALSE]>>
    /\ writeCtx' = "none"
    /\ UNCHANGED <<execPending>>

SysDeliversMessage ==
    \* event_loop.rs:1715 普通消息（鼠标/定时/焦点）同样在窗口过程里成帧
    /\ frames = <<>>
    /\ Len(frames) < MaxDepth              \* model bound; 调大 MaxDepth 只会拉长反例路径
    /\ frames' = <<[kind |-> "sys", handled |-> FALSE]>>
    /\ writeCtx' = "none"
    /\ UNCHANGED <<execPending>>

RunExec ==
    \* event_loop.rs:2838 消息泵取出 EXEC_MSG_ID 闭包执行，同样占一个帧
    /\ frames = <<>>
    /\ execPending > 0
    /\ frames' = <<[kind |-> "exec", handled |-> FALSE]>>
    /\ execPending' = execPending - 1
    /\ writeCtx' = "none"

(************************ 帧内：每帧一次 Slint 处理 ************************)

SlintIgnoresSys ==
    \* runner.rs:208 消息分发给 Slint；大多数消息不触发输入法更新
    /\ frames /= <<>>
    /\ frames[Len(frames)].kind = "sys"
    /\ ~frames[Len(frames)].handled
    /\ frames' = [frames EXCEPT ![Len(frames)].handled = TRUE]
    /\ writeCtx' = "none"
    /\ UNCHANGED <<execPending>>

SlintImeDefersWrite ==
    \* 修复后：text.rs:1773 update_ime -> winitwindowadapter.rs:2558
    \* input_method_request -> window.rs:924 defer_to_thread，帧内只投递不写
    /\ Fixed = TRUE                        \* event_loop.rs:867 defer_to_thread 永远走消息队列
    /\ frames /= <<>>
    /\ frames[Len(frames)].kind = "ime"
    /\ ~frames[Len(frames)].handled
    /\ frames' = [frames EXCEPT ![Len(frames)].handled = TRUE]
    /\ execPending' = IF execPending < MaxExec THEN execPending + 1 ELSE execPending
                                       \* event_loop.rs:874 队列满则丢弃，由下一次更新重新补齐
    /\ writeCtx' = "none"

SlintImeWritesInline ==
    \* 修复前：text.rs:1773 -> window.rs(修复前) 经 execute_in_thread
    \* （event_loop.rs:836 线程内内联执行）在输入法消息帧内直接写输入法；
    \* legacy IMM32 输入法在自家消息帧内收到写调用可能同步回弹一条组合消息
    \* （同线程 SendMessage 重入），重新进入窗口过程
    /\ Fixed = FALSE                       \* event_loop.rs:836 修复前内联执行
    /\ frames /= <<>>
    /\ frames[Len(frames)].kind = "ime"
    /\ ~frames[Len(frames)].handled
    /\ \/ frames' = [frames EXCEPT ![Len(frames)].handled = TRUE]
       \/ \/ Len(frames) < MaxDepth        \* model bound; 递归在此截断
          /\ frames' = Append([frames EXCEPT ![Len(frames)].handled = TRUE],
                              [kind |-> "ime", handled |-> FALSE])
    /\ writeCtx' = "under-ime"
    /\ UNCHANGED <<execPending>>

SlintSysDefersWrite ==
    \* 修复后：光标移动等普通消息触发的 update_ime 同样只投递（window.rs:924）
    /\ Fixed = TRUE                        \* event_loop.rs:867 defer_to_thread 永远走消息队列
    /\ frames /= <<>>
    /\ frames[Len(frames)].kind = "sys"
    /\ ~frames[Len(frames)].handled
    /\ frames' = [frames EXCEPT ![Len(frames)].handled = TRUE]
    /\ execPending' = IF execPending < MaxExec THEN execPending + 1 ELSE execPending
                                       \* event_loop.rs:874 队列满则丢弃
    /\ writeCtx' = "none"

SlintSysWritesInline ==
    \* 修复前：普通消息帧内直接写输入法；输入法未处于消息投递中时通常无害，
    \* 但它越过了“写只发生在泵轮闭包里”的边界；写调用也可能让输入法同步
    \* 回弹组合消息，随后在回弹帧里再次内联写，形成递归
    /\ Fixed = FALSE                       \* event_loop.rs:836 修复前内联执行
    /\ frames /= <<>>
    /\ frames[Len(frames)].kind = "sys"
    /\ ~frames[Len(frames)].handled
    /\ \/ frames' = [frames EXCEPT ![Len(frames)].handled = TRUE]
       \/ \/ Len(frames) < MaxDepth        \* model bound; 递归在此截断
          /\ frames' = Append([frames EXCEPT ![Len(frames)].handled = TRUE],
                              [kind |-> "ime", handled |-> FALSE])
    /\ writeCtx' = "sys-frame"
    /\ UNCHANGED <<execPending>>

SlintExecWrites ==
    \* ime.rs:145 延迟闭包在干净的泵轮帧里执行 ImmSetCompositionWindow/
    \* ImmSetCandidateWindow；写也可能让输入法同步回弹组合消息，回弹帧属于
    \* 受支持的重入路径，修复后它只投递、不再写。修复前后此路径相同
    /\ frames /= <<>>
    /\ frames[Len(frames)].kind = "exec"
    /\ ~frames[Len(frames)].handled
    /\ \/ frames' = [frames EXCEPT ![Len(frames)].handled = TRUE]
       \/ \/ Len(frames) < MaxDepth        \* model bound
          /\ frames' = Append([frames EXCEPT ![Len(frames)].handled = TRUE],
                              [kind |-> "ime", handled |-> FALSE])
    /\ writeCtx' = "exec"
    /\ UNCHANGED <<execPending>>

ReturnFromFrame ==
    \* runner.rs:229 事件处理返回，窗口过程出栈一帧
    /\ frames /= <<>>
    /\ frames[Len(frames)].handled
    /\ frames' = IF Len(frames) = 1 THEN <<>>
                 ELSE [i \in 1..Len(frames) - 1 |-> frames[i]]
    /\ writeCtx' = "none"
    /\ UNCHANGED <<execPending>>

(************************ 锁家族：窗口过程互斥锁 ***************************)

CaptureSyscallUnderLock ==
    \* 修复前：event_loop.rs:1048 capture_mouse 持 WindowState 锁调用 SetCapture，
    \* 同步送达的 WM_CAPTURECHANGED 重新进入窗口过程并再次取锁，
    \* std Mutex 不可重入（见经验教训：死锁家族第一例）
    /\ Fixed = FALSE                       \* event_loop.rs:1056 修复前守卫跨 SetCapture 存活
    /\ Len(frames) = 1
    /\ frames[Len(frames)].kind = "sys"    \* event_loop.rs:1715 按键处理帧内发生
    /\ frames' = Append(frames, [kind |-> "wedge", handled |-> FALSE])
    /\ writeCtx' = "none"
    /\ UNCHANGED <<execPending>>

CaptureSyscallAfterUnlock ==
    \* 修复后：event_loop.rs:1055 先放锁再 SetCapture；重入帧正常取锁、正常处理
    /\ Fixed = TRUE                        \* event_loop.rs:1055 修复后先 drop 守卫再 SetCapture
    /\ Len(frames) = 1
    /\ frames[Len(frames)].kind = "sys"    \* event_loop.rs:1715 按键处理帧内发生
    /\ Len(frames) < MaxDepth              \* model bound
    /\ frames' = Append(frames, [kind |-> "sys", handled |-> FALSE])
    /\ writeCtx' = "none"
    /\ UNCHANGED <<execPending>>

Next ==
    \/ ImeDeliversComposition
    \/ SysDeliversMessage
    \/ RunExec
    \/ SlintIgnoresSys
    \/ SlintImeDefersWrite
    \/ SlintImeWritesInline
    \/ SlintSysDefersWrite
    \/ SlintSysWritesInline
    \/ SlintExecWrites
    \/ ReturnFromFrame
    \/ CaptureSyscallUnderLock
    \/ CaptureSyscallAfterUnlock

(************************ 规则（读者语言的不变量） *************************)

TypeOK ==
    /\ frames \in FrameSeq
    /\ execPending \in 0..MaxExec
    /\ writeCtx \in {"none", "exec", "sys-frame", "under-ime"}

\* 输入法消息帧存活期间，本线程绝不再发起会回到输入法状态机的写调用。
NoImeWriteUnderImeFrame == writeCtx # "under-ime"

\* 会回到输入法的写调用只发生在消息泵轮的延迟闭包里，不占用任何系统消息帧。
ImeWriteAtFrameBoundary == writeCtx \in {"none", "exec"}

\* 帧栈深度有界：同步重入链条不得超过“一条消息 + 一次输入法回弹”。
FrameStackBounded == Len(frames) <= 2

\* 线程不会进入持锁重入的卡死吸收态。
NoLockWedge == ~WedgeOnStack

Spec == Init /\ [][Next]_vars
=============================================================================
