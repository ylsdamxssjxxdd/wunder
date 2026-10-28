export * from './ProcessRefreshFixed.map.mjs';
export const legend = { works: '原有规则成立', wrong: '仍存在失配', fixed: '修复后不再可达或新增正确状态' };
export function step(action, before) {
  if (action === 'ReloadTimerDuringRequest' && 'refreshPending' in before) return '读取在途，保留刷新';
  const labels = {
    WatchStructuralChange: '计数变化，安排刷新', ToolResultCommitted: '工具结果落库',
    WatchTerminalChange: '收到完成态，安排刷新', ReloadTimerStartsRequest: '延后 600 毫秒读取',
    ReloadTimerDuringRequest: '读取在途，丢弃刷新', LoadPageSettles: '旧响应落地', DrainPendingRefresh: '补读保留的刷新'
  };
  return labels[action] ?? action;
}
