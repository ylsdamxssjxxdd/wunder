const NAME = { a: '实例 A', b: '实例 B' };

const EXECUTOR_STATE = { idle: '空闲', open: '已打开线程', exec: '执行中' };

function lockText(lock) {
  if (!lock || lock.owner === 'none') return '无执行锁';
  const who = NAME[lock.owner] ?? lock.owner;
  return lock.alive ? `${who} 持有执行锁（第 ${lock.gen} 代）` : `执行锁已过期（原 ${who} 第 ${lock.gen} 代）`;
}

function leaseText(lease) {
  if (!lease || lease.owner === 'none') return '无线程打开租约';
  const who = NAME[lease.owner] ?? lease.owner;
  return lease.alive ? `${who} 打开线程（租约第 ${lease.tok} 代）` : `打开租约已过期（原 ${who} 第 ${lease.tok} 代）`;
}

function executorText(pc) {
  return ['a', 'b'].map((e) => `${NAME[e]}${EXECUTOR_STATE[pc?.[e]] ?? pc?.[e] ?? '未知'}`).join('，');
}

// 心跳不改变 pc；发起者从锁行代次与各执行者自认代次的对应关系推断。
function heartbeatMover(before, after) {
  const lock = after.lockRow ?? before.lockRow;
  if (after.foreignTouch && !before.foreignTouch) {
    const foreign = ['a', 'b'].find((e) => lock && (lock.owner !== e || lock.gen !== before.myGen?.[e]));
    return foreign ?? 'a';
  }
  return lock?.owner ?? 'a';
}

function pcMover(before, after) {
  return ['a', 'b'].find((e) => before.pc?.[e] !== after.pc?.[e]) ?? 'a';
}

export const readerActions = [];

export function state(values) {
  return `${lockText(values.lockRow)}<br/>${leaseText(values.leaseRow)}<br/>${executorText(values.pc)}`;
}

export function step(action, before, after) {
  const base = String(action).split('(')[0];
  if (base === 'AcquireLease') {
    const e = pcMover(before, after);
    return `${NAME[e]} 打开线程，获得打开租约（第 ${after.leaseRow?.tok} 代）`;
  }
  if (base === 'AcquireLock') {
    const e = pcMover(before, after);
    return `${NAME[e]} 开始执行，获得执行锁（第 ${after.lockRow?.gen} 代）`;
  }
  if (base === 'ExpireLock') return '执行锁持有者心跳延迟，锁行到期';
  if (base === 'ExpireLease') return '打开租约持有者心跳延迟，租约到期';
  if (base === 'HeartbeatLock') {
    const e = heartbeatMover(before, after);
    if (after.foreignTouch && !before.foreignTouch) return `${NAME[e]} 的迟到心跳给他人的执行锁续期`;
    return `${NAME[e]} 心跳续期自己的执行锁`;
  }
  if (base === 'ReleaseLock') {
    const e = pcMover(before, after);
    if (after.foreignRelease && !before.foreignRelease) return `${NAME[e]} 的迟到释放删掉了他人的执行锁`;
    return `${NAME[e]} 执行结束，释放执行锁`;
  }
  if (base === 'ReleaseLease') {
    const e = pcMover(before, after);
    return `${NAME[e]} 关闭线程，释放打开租约`;
  }
  return base;
}

export function cause(vars, facts, invariant) {
  if (String(invariant).includes('LateTouch')) return '执行锁 touch 只按线程匹配，旧持有者的迟到心跳给新持有者的锁行续期';
  return '执行锁 release 只按线程匹配，旧持有者的迟到释放删除了新持有者的锁行';
}

export const legend = {
  works: '规则成立',
  wrong: '状态失配',
  fixed: '修复后旧错误不可达或新增正确状态',
};
