export const readerActions = [];
export function state(values) {
  if (values.rowStatus === 'none') return '尚未发送';
  if (values.transportClosed && values.notified) return '传输已收尾，气泡仍运行，却提示完成';
  if (values.transportClosed) return '传输已收尾，气泡继续运行';
  if (values.latestStatus === 'running') return '线程与气泡运行';
  return values.rowStatus === 'running'
    ? '线程完成，气泡仍运行'
    : '线程与气泡完成';
}
const steps = {
  CommitThreadView: '受理后提交展示窗',
  TransportFinal: '请求传输先收尾',
  ApplyThreadViewHead: '目录先返回完成态',
  ApplyThreadViewChanges: '展示帧稍后到达',
  LateRunningReplay: '同轮次迟到运行快照到达'
};
export function step(action) { return steps[action] ?? action; }
export function cause(values) {
  if (values.transportClosed && values.notified) return '无轮次身份的传输收尾被当作任务完成';
  if (values.completedSeen && values.latestStatus === 'running') return '同轮次迟到运行快照重新打开终态';
  return '目录只推进线程头，没有同时更新对应气泡';
}
export function group(values) { return values.latestStatus === 'completed' ? '已知线程完成' : '发送与执行'; }
export const legend = { works:'规则成立', wrong:'状态失配', after:'反例后的状态' };
