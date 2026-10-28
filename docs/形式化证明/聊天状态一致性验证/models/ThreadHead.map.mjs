export const readerActions = [];
export function state(values) {
  if (values.rowStatus === 'none') return '尚未发送';
  if (values.latestStatus === 'running') return '线程与气泡运行';
  return values.rowStatus === 'running'
    ? '线程完成，气泡仍运行'
    : '线程与气泡完成';
}
const steps = {
  CommitThreadView: '受理后提交展示窗',
  ApplyThreadViewHead: '目录先返回完成态',
  ApplyThreadViewChanges: '展示帧稍后到达'
};
export function step(action) { return steps[action] ?? action; }
export function cause() { return '目录只推进线程头，没有同时更新对应气泡'; }
export function group(values) { return values.latestStatus === 'completed' ? '已知线程完成' : '发送与执行'; }
export const legend = { works:'规则成立', wrong:'状态失配', after:'反例后的状态' };
