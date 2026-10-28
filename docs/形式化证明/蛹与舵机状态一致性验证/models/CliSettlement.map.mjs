export const readerActions = ['ToolCall', 'FailedTerminal'];
export function state(values) {
  const phases = ['请求已受理','工具已启动','执行已失败，订阅待结束','订阅已结束'];
  const states = {working:'线程工作中',failed:'线程失败',finished:'线程已完成'};
  const tools = {none:'尚无工具',running:'工具转圈',interrupted:'工具已中断'};
  return `${phases[values.phase]}<br/>${states[values.status]}，${tools[values.tool]}`;
}
export function step(action) { return {ToolCall:'开始工具调用',FailedTerminal:'收到执行失败终态',StreamDone:'订阅结束，结算展示'}[action] ?? action; }
export function cause() { return '执行失败终态被忽略，订阅结束又被当成成功，工具未结清'; }
export const legend = {works:'规则成立',wrong:'状态失配',fixed:'修复后旧错误不可达或新增正确状态'};
