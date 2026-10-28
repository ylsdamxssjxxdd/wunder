export const readerActions = ['Open', 'Claim', 'DropHandle', 'Exit'];
const names = {'desktop':'蛹','cli-a':'舵机甲','cli-b':'舵机乙','thread-a':'任务甲','thread-b':'任务乙','none':'无'};
export function state(values) {
  const opened = values.opened.map(name => names[name]).join('、') || '无';
  const rejected = values.rejected.map(name => names[name]).join('、') || '无';
  const owners = Object.entries(values.owners).map(([thread, owner]) => `${names[thread]}：${names[owner]}`).join('<br/>');
  return `已启动：${opened}<br/>启动被拦：${rejected}<br/>${owners}<br/>排空中的写入：${values.drains.length}`;
}
export function step(action) {
  const name = String(action).split('(')[0];
  return ({Open:'打开本地入口',Claim:'领取线程写入权',DropHandle:'关闭前台句柄，后台继续排空',FinishDrain:'写入排空并释放线程',Exit:'本地进程退出'})[name] ?? String(action);
}
export function cause() { return '线程已经隔离，但数据根的进程级独占仍拒绝其他本地入口'; }
