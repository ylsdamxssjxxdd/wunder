export const readerActions = [];
export function state(value) {
  const title = { refresh: '连续结构事件', tool: '工具实时条目', order: '并发工具顺序' }[value.scenario];
  if (value.scenario === 'refresh') return `${title}<br/>已收到${value.count}次事件<br/>首次期限${value.firstDeadline}，当前期限${value.deadline}<br/>${value.requestStarted ? '请求已发出' : '等待刷新'}`;
  if (value.scenario === 'order') return `${title}<br/>${value.phase === 0 ? '尚未启动' : value.phase === 1 ? '依次启动工具一、二' : '工具二先完成'}<br/>展示顺序${Array.isArray(value.order) ? value.order.join('、') : JSON.stringify(value.order)}`;
  return `${title}<br/>${value.phase === 0 ? '尚未启动' : value.phase === 1 ? '执行中' : value.phase === 2 ? '结果已到达' : '收到迟到启动帧'}<br/>${value.visibleTool ? '条目已展示' : '未展示条目'}，${value.processItemsStatus === 'running' ? '转圈' : '已完成'}`;
}
export function step(action) { return { StructuralEvent: '新事件不推迟首次刷新期限', DeadlineFires: '期限到达，发起读取', ToolStarts: '工具开始即推送条目', ToolFinishes: '收到工具结果', LateToolStart: '拒绝迟到启动帧', ConcurrentToolStarts: '记录两次启动的稳定序号', ReversedToolCompletion: '逆序完成，不改变启动顺序' }[action] || action; }
export function group(value) { return value.scenario; }
export function cause(value) { return value.scenario; }
