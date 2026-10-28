const NAME = { k1: '消息 K1', k2: '消息 K2' };
const KEYS = ['k1', 'k2'];

const STATUS = {
  accepted: '已受理',
  queued: '排队中',
  running: '执行中',
  completed: '已完成',
  cancelled: '已取消',
  interrupted: '已中断',
};
const TASK = {
  none: '无任务',
  pending: '排队任务待派发',
  running: '任务执行中',
  stuck: '任务卡死在执行中',
  dead: '任务已终结',
};

function admissionText(values) {
  return KEYS.map((k) => {
    const row = values.adm?.[k];
    if (!row || row === 'none' || row.status === 'none') return `${NAME[k]}：未受理`;
    const extra = [];
    const task = values.task?.[k];
    if (task && task !== 'none') extra.push(TASK[task] ?? task);
    if (values.materialized?.[k]) extra.push('用户消息已写入历史');
    if (values.clientAck?.[k]) extra.push('客户端已收到受理结论');
    if (values.sawDuplicate?.[k]) extra.push('客户端看到「已受理」拒绝');
    if (values.sawAltered?.[k]) extra.push('同键不同内容被先写者胜出');
    const suffix = extra.length ? `（${extra.join('，')}）` : '';
    return `${NAME[k]}：${STATUS[row.status] ?? row.status}，第 ${row.round} 轮${suffix}`;
  });
}

export const readerActions = [];

export function state(values) {
  const exec = values.executorUp
    ? values.lockLive
      ? '执行进程存活，持有会话锁'
      : '执行进程存活'
    : values.lockLive
      ? '进程已崩溃，会话锁行等待过期'
      : '执行进程已崩溃';
  const stale = values.monitorStale ? '<br/>监控记录停留在「运行中」（崩溃残留）' : '';
  const recovered = values.recovered ? '<br/>重启后的恢复已完成' : '';
  return `${admissionText(values).join('<br/>')}<br/>${exec}${stale}${recovered}<br/>已分配用户轮次：${values.latestRound}`;
}

function mover(before, after) {
  return (
    KEYS.find((k) => JSON.stringify(before.adm?.[k]) !== JSON.stringify(after.adm?.[k])) ??
    KEYS.find((k) => before.task?.[k] !== after.task?.[k]) ??
    KEYS.find((k) => before.submitInflight?.[k] !== after.submitInflight?.[k]) ??
    KEYS.find((k) => before.withdrawInflight?.[k] !== after.withdrawInflight?.[k]) ??
    KEYS.find((k) => before.clientAck?.[k] !== after.clientAck?.[k]) ??
    'k1'
  );
}

export function step(action, before, after) {
  const base = String(action).split('(')[0];
  const k = mover(before, after);
  const name = NAME[k];
  switch (base) {
    case 'AcceptNew':
      return `发送${name}：受理事务提交，分配第 ${after.adm?.[k]?.round} 轮用户轮次`;
    case 'DeliverAccept':
      return `${name}的受理响应送达客户端`;
    case 'AcceptReplay':
      return `响应丢失后重发${name}：命中受理库去重，返回原轮次（不再分配轮次）`;
    case 'AcceptReplayAltered':
      return `${name}同键不同内容重发：先写者胜，客户端收到「已受理」拒绝`;
    case 'Enqueue':
      return `${name}进入持久队列（受理行转排队中）`;
    case 'StartDirectRun':
      return `${name}不忙时直跑：获得会话锁`;
    case 'Claim':
      return `派发器领取${name}的排队任务，执行开始`;
    case 'BeginTurn':
      return `${name}的用户消息写入历史，受理行转执行中`;
    case 'FinishExecution':
      return `${name}执行完成，受理行终态化并释放会话锁`;
    case 'WithdrawTask':
      return `撤下${name}：排队任务先置为已取消`;
    case 'SettleWithdrawnTurn':
      return `撤下${name}：受理行终态化为已取消`;
    case 'Crash':
      return '进程崩溃（在途的提交/撤下续程丢失，运行中的任务行留在执行中）';
    case 'LockExpire':
      return '会话锁 TTL 到期（崩溃进程的心跳已停止）';
    case 'Restart':
      return '进程重启，拉起队列循环';
    case 'SettleOrphan':
      return `结算${name}的孤儿轮次为已中断（无存活任务与执行锁）`;
    default:
      return base;
  }
}

export function cause(vars, facts, invariant) {
  if (String(invariant).includes('QueuedInputNotStarved')) {
    return '崩溃残留的「运行中」监控记录永久挡住派发：重启后排队中的已受理输入不会再被执行，也不被结算';
  }
  return '受理落库与排队/执行之间没有崩溃恢复：孤儿轮次停留在非终态，没有任何动作再驱动或结算它';
}

export const legend = {
  works: '规则成立',
  wrong: '状态失配',
  fixed: '修复后旧错误不可达或新增正确状态',
};
