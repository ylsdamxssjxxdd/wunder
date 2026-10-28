--------------------------- MODULE MessageAdmission ---------------------------
\* FV-02 旧行为模型：消息受理幂等、重试去重、用户轮次分配与重启后的孤儿轮次。
\* 基线提交 508e12a（桌面启动只拉起队列循环，不再调用 recover_desktop_runtime_state）。
\* 单任务线程、两条独立客户端消息（k1/k2，各自持有 client_message_id）。
\* 受理是单个 Immediate 事务的原子提交；响应送达客户端是事务之后的独立一步，
\* 两者之间允许崩溃。执行锁活性抽象为 lockLive（TTL 心跳期间为真）。
EXTENDS Integers, FiniteSets
CONSTANTS Keys, MaxRound
VARIABLES adm, latestRound, allocations, task, materialized,
          executorUp, lockLive, monitorStale, submitInflight, withdrawInflight,
          clientAck, sawDuplicate, sawAltered, recovered
vars == <<adm, latestRound, allocations, task, materialized,
          executorUp, lockLive, monitorStale, submitInflight, withdrawInflight,
          clientAck, sawDuplicate, sawAltered, recovered>>

NoRow == [round |-> 0, status |-> "none"]
LiveStatus == {"accepted", "queued", "running"}
LiveTask == {"pending", "running"}

Init ==
    /\ adm = [k \in Keys |-> NoRow]
    /\ latestRound = 0
    /\ allocations = 0
    /\ task = [k \in Keys |-> "none"]
    /\ materialized = [k \in Keys |-> FALSE]
    /\ executorUp = TRUE
    /\ lockLive = FALSE
    /\ monitorStale = FALSE
    /\ submitInflight = [k \in Keys |-> FALSE]
    /\ withdrawInflight = [k \in Keys |-> FALSE]
    /\ clientAck = [k \in Keys |-> FALSE]
    /\ sawDuplicate = [k \in Keys |-> FALSE]
    /\ sawAltered = [k \in Keys |-> FALSE]
    /\ recovered = FALSE

\* 新受理。crates/wunder-runtime/src/storage/thread_admission.rs:513-589
\* 单个 Immediate 事务：查重未命中(:563-570) 后分配用户轮次(:577-584)、
\* 插入受理行(:586)、提交(:587)。响应送达是事务之后的另一步，不在本动作内。
AcceptNew(k) ==
    \* thread_admission.rs:564（按 thread+client_message_id 查重未命中才进入分配）
    /\ adm[k].status = "none"
    /\ allocations < MaxRound
    /\ allocations' = allocations + 1
    \* thread_admission.rs:577（UPDATE latest_user_round+1 与受理行同事务）
    /\ latestRound' = allocations'
    /\ adm' = [adm EXCEPT ![k] = [round |-> allocations', status |-> "accepted"]]
    /\ submitInflight' = [submitInflight EXCEPT ![k] = TRUE]
    /\ UNCHANGED <<task, materialized, executorUp, lockLive, monitorStale,
                   withdrawInflight, clientAck, sawDuplicate, sawAltered, recovered>>

\* 受理响应送达客户端。runtime.rs:343-352 把受理结果返回给调用方。
DeliverAccept(k) ==
    \* thread_admission.rs:566-568,588（只有已受理的键才有受理结论可送达）
    /\ adm[k].status # "none"
    \* 只有尚未收到受理结论的客户端才会等待这一步（响应丢失时它转而重发）
    /\ clientAck[k] = FALSE
    /\ clientAck' = [clientAck EXCEPT ![k] = TRUE]
    /\ UNCHANGED <<adm, latestRound, allocations, task, materialized,
                   executorUp, lockLive, monitorStale, submitInflight,
                   withdrawInflight, sawDuplicate, sawAltered, recovered>>

\* 同键重发：受理库查重命中，返回既有轮次 created:false，不再分配轮次。
\* thread_admission.rs:563-570；runtime.rs:348-350 把 created:false 转为可观察的拒绝。
AcceptReplay(k) ==
    \* thread_admission.rs:564（查重命中）
    /\ adm[k].status # "none"
    \* thread_admission.rs:566-568（返回首轮受理身份，created:false）
    /\ clientAck[k] = FALSE
    /\ clientAck' = [clientAck EXCEPT ![k] = TRUE]
    /\ sawDuplicate' = [sawDuplicate EXCEPT ![k] = TRUE]
    /\ UNCHANGED <<adm, latestRound, allocations, task, materialized,
                   executorUp, lockLive, monitorStale, submitInflight,
                   withdrawInflight, sawAltered, recovered>>

\* 同键不同内容：查重只认键不认内容，先写者胜，调用方收到 created:false。
\* thread_admission.rs:563-570 的查询条件不含正文。
AcceptReplayAltered(k) ==
    \* thread_admission.rs:564（同键命中，不比较正文）
    /\ adm[k].status # "none"
    /\ sawDuplicate' = [sawDuplicate EXCEPT ![k] = TRUE]
    /\ sawAltered' = [sawAltered EXCEPT ![k] = TRUE]
    /\ clientAck' = [clientAck EXCEPT ![k] = TRUE]
    /\ UNCHANGED <<adm, latestRound, allocations, task, materialized,
                   executorUp, lockLive, monitorStale, submitInflight,
                   withdrawInflight, recovered>>

\* 受理成功后建立排队任务。runtime.rs:416-428（enqueue_task），
\* 排队披露把受理行置为 queued：runtime.rs:1142-1147。
Enqueue(k) ==
    \* runtime.rs:343-347（同一次提交流程内，受理提交后才排队；崩溃即丢失续程）
    /\ submitInflight[k]
    /\ adm[k].status # "none"
    /\ adm[k].status = "accepted"
    \* runtime.rs:1000-1016（每个受理至多建立一个 agent task）
    /\ task[k] = "none"
    /\ task' = [task EXCEPT ![k] = "pending"]
    /\ adm' = [adm EXCEPT ![k].status = "queued"]
    /\ submitInflight' = [submitInflight EXCEPT ![k] = FALSE]
    /\ UNCHANGED <<latestRound, allocations, materialized, executorUp, lockLive,
                   monitorStale, withdrawInflight, clientAck, sawDuplicate,
                   sawAltered, recovered>>

\* 直跑路径：不忙时受理后不建任务，直接占启动租约并随执行获得会话锁。
\* runtime.rs:421-423；crates/wunder-runtime/src/orchestrator/execute.rs:224-233。
StartDirectRun(k) ==
    /\ submitInflight[k]
    \* runtime.rs:411-429（排队/直跑决策只发生在受理成功之后）
    /\ adm[k].status # "none"
    \* thread_admission.rs:622-635（can_transition：accepted 方可进入 running）
    /\ adm[k].status = "accepted"
    \* runtime.rs:1000-1016（每个受理至多建立一个 agent task；直跑路径不建任务）
    /\ task[k] = "none"
    \* execute.rs:224-233（执行前先获得会话锁，无锁不执行）
    /\ executorUp
    \* execute.rs:88-93 的存储冲突语义（锁被他人持有时 UserBusy，不建模自旋）
    /\ ~lockLive
    /\ lockLive' = TRUE
    /\ submitInflight' = [submitInflight EXCEPT ![k] = FALSE]
    /\ UNCHANGED <<adm, latestRound, allocations, task, materialized, executorUp,
                   monitorStale, withdrawInflight, clientAck, sawDuplicate,
                   sawAltered, recovered>>

\* 派发器领取排队任务并开始执行。runtime.rs:1639-1644（claim_agent_task），
\* 执行本体在 execute.rs:224-233 获得会话锁。
\* 旧行为：runtime.rs:1331-1337（is_session_idle 只看监控记录状态），
\* 崩溃残留的 running 监控记录永久挡住派发。
Claim(k) ==
    \* agent_runtime_store.rs:158（只领取 pending/retry 任务）
    /\ task[k] = "pending"
    \* runtime.rs:1639-1644（派发循环运行在存活的执行进程内）
    /\ executorUp
    \* runtime.rs:1331-1337（监控记录 running/cancelling 即视为不空闲，无锁活性仲裁）
    /\ ~monitorStale
    \* runtime.rs:1339（持有会话锁视为不空闲）
    /\ ~lockLive
    /\ task' = [task EXCEPT ![k] = "running"]
    /\ lockLive' = TRUE
    /\ UNCHANGED <<adm, latestRound, allocations, materialized, executorUp,
                   monitorStale, submitInflight, withdrawInflight, clientAck,
                   sawDuplicate, sawAltered, recovered>>

\* 执行开始：物化用户消息并把受理行置 running。begin_turn 幂等：
\* thread_store.rs:676-684（用户消息已存在则跳过追加）。
BeginTurn(k) ==
    \* execute.rs:224-233（执行本体运行在存活进程内）
    /\ executorUp
    \* execute.rs:224-233（执行全程持有存活会话锁，心跳 5s 续期）
    /\ lockLive
    /\ adm[k].status # "none"
    \* thread_admission.rs:622-635（accepted/queued 方可转入 running）
    /\ adm[k].status \in {"accepted", "queued"}
    \* thread_store.rs:676-684（每轮至多物化一次用户消息）
    /\ materialized[k] = FALSE
    /\ materialized' = [materialized EXCEPT ![k] = TRUE]
    /\ adm' = [adm EXCEPT ![k].status = "running"]
    /\ UNCHANGED <<latestRound, allocations, task, executorUp, monitorStale,
                   submitInflight, withdrawInflight, clientAck, sawDuplicate,
                   sawAltered, recovered, lockLive>>

\* 执行完成：受理行终态化，任务结清，释放锁。
\* runtime.rs:1856-1868（任务成功写回）与 committer 的 completed 更新。
FinishExecution(k) ==
    \* runtime.rs:1856-1868（完成写回由存活的执行进程自己进行）
    /\ executorUp
    /\ lockLive
    /\ adm[k].status # "none"
    \* thread_admission.rs:622-635（只有 running 等活态能进入 completed）
    /\ adm[k].status = "running"
    /\ adm' = [adm EXCEPT ![k].status = "completed"]
    /\ task' = [task EXCEPT ![k] = "dead"]
    /\ lockLive' = FALSE
    /\ UNCHANGED <<latestRound, allocations, materialized, executorUp,
                   monitorStale, submitInflight, withdrawInflight, clientAck,
                   sawDuplicate, sawAltered, recovered>>

\* 撤下排队轮次第一步：任务置 cancelled（终态单调）。
\* queue_tasks.rs:238-252；agent_runtime_store.rs:287-297（终态不可回退）。
WithdrawTask(k) ==
    \* queue_tasks.rs:11-13,154（只有 parked 的 pending/retry 任务可撤）
    /\ task[k] = "pending"
    /\ task' = [task EXCEPT ![k] = "dead"]
    /\ withdrawInflight' = [withdrawInflight EXCEPT ![k] = TRUE]
    /\ UNCHANGED <<adm, latestRound, allocations, materialized, executorUp,
                   lockLive, monitorStale, submitInflight, clientAck,
                   sawDuplicate, sawAltered, recovered>>

\* 撤下第二步：受理行终态化 cancelled。queue_tasks.rs:258-281。
\* 与上一步之间不是同一事务，中间允许崩溃。
SettleWithdrawnTurn(k) ==
    /\ withdrawInflight[k]
    \* queue_tasks.rs:270-281（撤下结算由存活进程执行）
    /\ executorUp
    /\ adm[k].status # "none"
    \* thread_admission.rs:622-635（accepted/queued 可转入 cancelled 终态）
    /\ adm[k].status \in {"accepted", "queued"}
    /\ adm' = [adm EXCEPT ![k].status = "cancelled"]
    /\ withdrawInflight' = [withdrawInflight EXCEPT ![k] = FALSE]
    /\ UNCHANGED <<latestRound, allocations, task, materialized, executorUp,
                   lockLive, monitorStale, submitInflight, clientAck,
                   sawDuplicate, sawAltered, recovered>>

\* 进程崩溃：内存中的提交/撤下续程丢失；运行中的任务行停留在 running
\*（重启后不会被再派发：agent_runtime_store.rs:158 只列出 pending/retry）；
\* 监控记录持久化为 running（ops/monitor.rs 的 to_storage，无启动和解）。
Crash ==
    /\ executorUp
    /\ executorUp' = FALSE
    /\ submitInflight' = [k \in Keys |-> FALSE]
    /\ withdrawInflight' = [k \in Keys |-> FALSE]
    /\ task' = [k \in Keys |-> IF task[k] = "running" THEN "stuck" ELSE task[k]]
    \* 崩溃瞬间执行中 ⇒ 监控记录停在 running（锁行要等 TTL 才失效）
    /\ monitorStale' = lockLive
    /\ recovered' = FALSE
    /\ UNCHANGED <<adm, latestRound, allocations, materialized, lockLive,
                   clientAck, sawDuplicate, sawAltered>>

\* 会话锁行 TTL 到期（心跳已随进程停止）。
\* session_lock_store.rs:55-60（获取方事务里物理删除过期行）。
LockExpire ==
    \* orchestrator/constants.rs:14-16（TTL=120s、心跳 5s 随进程停止，锁行才可能到期）
    /\ ~executorUp
    /\ lockLive
    /\ lockLive' = FALSE
    /\ UNCHANGED <<adm, latestRound, allocations, task, materialized, executorUp,
                   monitorStale, submitInflight, withdrawInflight, clientAck,
                   sawDuplicate, sawAltered, recovered>>

\* 重启。旧行为：crates/wunder-desktop/runtime.rs:358-363 只拉起队列循环，
\* 明确"启动期间绝不清除其他实例的锁或任务"，也没有孤儿受理的结算动作。
Restart ==
    /\ ~executorUp
    /\ executorUp' = TRUE
    /\ recovered' = TRUE
    /\ UNCHANGED <<adm, latestRound, allocations, task, materialized, lockLive,
                   monitorStale, submitInflight, withdrawInflight, clientAck,
                   sawDuplicate, sawAltered>>

SystemStep ==
    \/ \E k \in Keys: Enqueue(k) \/ StartDirectRun(k) \/ Claim(k) \/ BeginTurn(k)
        \/ FinishExecution(k) \/ SettleWithdrawnTurn(k)
    \/ LockExpire

Next ==
    \/ \E k \in Keys: AcceptNew(k) \/ DeliverAccept(k) \/ AcceptReplay(k)
        \/ AcceptReplayAltered(k) \/ WithdrawTask(k)
    \/ SystemStep
    \/ Crash \/ Restart

TypeOK ==
    /\ \A k \in Keys:
        /\ adm[k].status = "none"
           \/ (adm[k].round \in 1..MaxRound
               /\ adm[k].status \in {"accepted", "queued", "running",
                                     "completed", "cancelled", "interrupted", "none"})
        /\ task[k] \in {"none", "pending", "running", "stuck", "dead"}
        /\ materialized[k] \in BOOLEAN
    /\ latestRound \in 0..MaxRound
    /\ allocations \in 0..MaxRound
    /\ executorUp \in BOOLEAN /\ lockLive \in BOOLEAN /\ monitorStale \in BOOLEAN
    /\ recovered \in BOOLEAN

\* 规则1：用户轮次只随新受理分配——重放不分配，受理行数与已分配轮次恒等。
\* 变更点：thread_admission.rs:563-587。
RoundsMatchAdmissions ==
    /\ latestRound = allocations
    /\ allocations = Cardinality({k \in Keys : adm[k].status # "none"})

\* 规则2：独立用户消息不被错误合并——两个键各有自己的轮次。
\* 变更点：thread_admission.rs:571-586（每键独立分配）。
DistinctMessagesNotMerged ==
    \A a, b \in Keys :
        (a # b /\ adm[a].status # "none" /\ adm[b].status # "none")
            => adm[a].round # adm[b].round

\* 规则3：先写者胜必须可观察——同键不同内容永远伴随 created:false 结论返回，
\* 存储正文不被替换（本模型中受理行从未被重写，规则即该可观察性）。
FirstWriteWinsObservable ==
    \A k \in Keys : sawAltered[k] => sawDuplicate[k]

\* 孤儿：非终态受理，既没有存活任务/在途续程，也没有存活执行锁，永远不会再被驱动。
Orphan(k) ==
    /\ adm[k].status # "none"
    /\ adm[k].status \in LiveStatus
    /\ task[k] \in {"none", "dead", "stuck"}
    /\ ~lockLive
    /\ ~submitInflight[k]
    /\ ~withdrawInflight[k]

\* 系统动作沉寂点：客户端/环境动作（发送、重发、崩溃、重启）不算。
Quiescent == ~ENABLED SystemStep

\* 规则4：孤儿轮次必须被结算——重启恢复完成且系统沉寂后，不允许存在
\* 无存活任务、无存活执行锁的非终态受理（它永远不会再被驱动）。
\* 变更点：crates/wunder-desktop/runtime.rs:358-363 的启动序列（无孤儿结算）。
OrphanedInputSettles ==
    (Quiescent /\ recovered) => ~\E k \in Keys : Orphan(k)

\* 规则5：排队中的已受理输入不能被永久饿死——重启恢复完成且系统沉寂后，
\* 仍有存活排队任务的非终态受理必须能被派发（陈旧监控记录不得永久挡住派发）。
\* 变更点：runtime.rs:1331-1339 的空闲判定。
StarvedQueued(k) ==
    /\ adm[k].status # "none"
    /\ adm[k].status \in LiveStatus
    /\ task[k] = "pending"
QueuedInputNotStarved ==
    (Quiescent /\ recovered) => ~\E k \in Keys : StarvedQueued(k)

Spec == Init /\ [][Next]_vars
=============================================================================
