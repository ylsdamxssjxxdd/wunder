export const readerActions = ['WatchStructuralChange', 'ToolResultCommitted', 'WatchTerminalChange'];
export function state(values) {
  const turn = values.status === 'completed' ? '气泡完成' : '气泡运行';
  const tool = values.processItemsStatus === 'completed' ? '工具勾选' : '工具转圈';
  const io = values.loadAbort ? '读取中' : '无读取';
  const timer = values.reloadTimer ? '待刷新' : '无刷新';
  const durable = values.toolStatus === 'completed' ? '工具已落库' : '工具未完成';
  return `${turn}，${tool}<br/>${io}，${timer}<br/>${durable}`;
}
const labels = {
  WatchStructuralChange: '计数变化，安排刷新',
  ToolResultCommitted: '工具结果落库',
  WatchTerminalChange: '收到完成态，安排刷新',
  ReloadTimerStartsRequest: '延后 600 毫秒读取',
  ReloadTimerDuringRequest: '读取在途，丢弃刷新',
  LoadPageSettles: '旧响应落地'
};
export function step(action) { return labels[action] ?? action; }
export function cause() { return '终态刷新被在途读取吞掉，旧响应落地后没有后续刷新'; }
export function group(values) { return values.status === 'completed' ? '终态已到达' : '执行中'; }
export const legend = { works:'规则成立', wrong:'状态失配' };
