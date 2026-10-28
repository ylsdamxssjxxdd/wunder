export const readerActions = ['CancelAncestor', 'DispatchFromSibling', 'ReuseWorker'];
export function state(values) {
  const worker = {none:'未派生',running:'执行中',completed:'已完成',cancelled:'已取消'};
  return `${values.ancestorCancelled ? '持久祖先已取消' : '持久祖先允许派生'}<br/>子任务：${worker[values.worker]}<br/>执行代次：${values.generation}${values.lateRelease ? '<br/>旧清理已到达' : ''}${values.escaped ? '<br/>取消后仍派生了新任务' : ''}`;
}
export function step(action) {
  return ({CancelAncestor:'取消祖先及已登记后代',DispatchFromSibling:'兄弟调度器派生祖先下的任务',FinishWorker:'子任务完成',ReuseWorker:'同线程登记新代执行',DropOldGuard:'旧执行清理迟到'})[String(action).split('(')[0]] ?? String(action);
}
export function cause() { return '只核对直接调度者，遗漏持久派生树中的已取消祖先'; }
