export const readerActions = ["Begin"];
export function state(value) {
  const driver = value.ioIndependent ? "独立网络驱动" : "依附业务线程的网络驱动";
  if (value.completed.length === 4) return "全部数据库请求完成";
  if (value.workerWaiters.length === 2 && value.blockingWaiters.length === 2 && !value.ready.length) return `${driver}：业务与替补线程全部等待数据库`;
  return `${driver}：业务等待 ${value.workerWaiters.length}，替补等待 ${value.blockingWaiters.length}，响应就绪 ${value.ready.length}，完成 ${value.completed.length}`;
}
export function step(action) {
  return { Begin: "调用同步数据库接口", Handoff: "移交业务线程继续调度", Poll: "网络驱动收到数据库响应", FinishWorker: "原业务线程完成查询", FinishBlocking: "替补线程完成查询" }[action] || action;
}
export function cause(value, facts, invariant) {
  return { NoIoStarvation: "网络驱动依附已耗尽的业务运行时", ExactCompletion: "请求完成状态不一致" }[invariant];
}
export const legend = { works: "可继续处理", wrong: "网络驱动饥饿", fixed: "独立 I/O 后可继续处理" };
