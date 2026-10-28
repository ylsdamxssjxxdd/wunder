const TURN = { run: '执行中', done: '已完成', stop: '已停止', failed: '已失败' };
const MONITOR = { run: '执行中', halt: '停止中', done: '已完成', stop: '已停止', error: '已失败' };
const FRAME = { none: '', done: '完成', stop: '停止', failed: '失败' };
const GOAL = { none: '', done: '完成', stop: '中止', failed: '失败' };

function turnText(v) {
  return `轮次终态：${TURN[v.turn] ?? v.turn}`;
}

function monitorText(v) {
  const base = `监视器：${MONITOR[v.monitor] ?? v.monitor}`;
  return v.cancelVisible ? `${base}，取消已可见` : base;
}

function frameText(v) {
  if (v.frame === 'none' && v.goal === 'none') return '终态帧：未发';
  const frame = FRAME[v.frame] ?? v.frame;
  const goal = GOAL[v.goal] ?? v.goal;
  return `终态帧：${frame || '未发'}，目标结局：${goal || '未报'}`;
}

function commandText(v) {
  if (v.command === 'none') return '外部命令：无';
  return v.command === 'run' ? '外部命令：运行中' : '外部命令：已结束';
}

function stopperText(v) {
  switch (v.stopper) {
    case 'idle': return '停止管线：未发起';
    case 'asked': return '停止管线：已请求';
    case 'fenced': return '停止管线：已受理（取消可见）';
    case 'cut': return '停止管线：轮次已取消';
    case 'killed': return '停止管线：命令已结束';
    case 'over': return '停止管线：本生命周期已关闭';
    default: return `停止管线：${v.stopper}`;
  }
}

export const readerActions = ['RequestStop', 'Reopen'];

export function state(v) {
  if (v.reopened) return '已结清并重开新轮次<br/>（旧轮次的取消标志已全部复位）';
  return [turnText(v), monitorText(v), frameText(v), commandText(v), stopperText(v)].join('<br/>');
}

export function step(action, before, after) {
  const base = String(action).split('(')[0];
  switch (base) {
    case 'RequestStop': return '用户按下停止';
    case 'FenceAndMonitor':
      return after.monitor === 'halt' && before.monitor !== 'halt'
        ? '停止受理：执行围栏置位，监视器进入停止中'
        : '停止受理：执行围栏置位（监视器已是终态，保持不变）';
    case 'CancelTurns':
      return before.turn === 'run' ? '持久控制面把活跃轮次写为已停止' : '持久控制面：轮次已是终态，写入被拒';
    case 'KillCommands':
      if (before.command === 'run') return '外部命令进程被熔断并杀掉';
      return after.monitor !== before.monitor ? '执行体已不在，取消方补写监视器终态' : '无外部命令需要结束';
    case 'PassLastCheckpoint': return '执行体通过最后一个取消检查点，进入收尾';
    case 'CheckpointSeesCancel': return '执行体在检查点看到取消，按已停止结清';
    case 'FinishSeesCancel': return '收尾前置取消闸命中：按已停止结清，不进入完成提交';
    case 'FinishCheck': return '收尾前置取消闸通过，进入完成提交窗口';
    case 'FinishCommit': return '收尾：完成写入被采纳，发出完成语义';
    case 'FinishCommitRejected': return '收尾：持久层拒绝完成写入，改按已停止结清';
    case 'FinishSuccess':
      if (before.turn !== 'run' && after.turn === before.turn) {
        return '收尾：持久层拒绝完成写入，但仍发出完成语义（终态帧/监视器/目标结局）';
      }
      return '收尾：完成写入被采纳，发出完成语义';
    case 'HitTimeout': return '执行超时，按失败结清';
    case 'Reopen': return '停止结清后，用户再次发言（新轮次）';
    default: return base;
  }
}

export function cause(vars, facts, invariant) {
  const name = String(invariant);
  if (name.includes('SettledViewsAgree')) return '停止受理后收尾仍写监视器为已完成，且没有任何环节把它改回已停止';
  if (name.includes('CancelNeverPresented')) return '收尾不读持久层写入结果，轮次已停止仍发出完成语义';
  if (name.includes('CommandsDead')) return '外部命令未随结清结束';
  if (name.includes('StopCompletes')) return '停止管线未走完';
  return '停止语义失配';
}

export const legend = {
  works: '规则成立',
  wrong: '状态失配',
  fixed: '修复后旧错误不可达或新增正确状态',
};
