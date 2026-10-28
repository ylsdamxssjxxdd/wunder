export const readerActions = ['ToolCall', 'CancelTerminal', 'FailedTerminal', 'LateFinal'];
export function state(values) {
  const phases = ['轮次开始', '工具运行', '收到取消终态', '收到失败终态', '失败后收到迟到回复', '取消流已结束', '失败流已结束'];
  const states = {running:'气泡运行',cancelled:'气泡已停止',failed:'气泡失败',completed:'气泡完成'};
  const tools = {none:'尚无工具',running:'工具转圈',interrupted:'工具已中断'};
  return `${phases[values.phase]}<br/>${states[values.state]}，${tools[values.tool]}`;
}
export function step(action) { return {ToolCall:'开始工具调用',CancelTerminal:'收到取消终态',FailedTerminal:'收到失败终态',LateFinal:'迟到最终回复',FinishCancelled:'取消订阅结束',FinishFailed:'失败订阅结束'}[action] ?? action; }
export function cause(values) { return values.phase === 5 ? '取消未被识别为非成功结算，工具仍转圈' : '失败终态没有保护，迟到回复覆盖为完成'; }
export const legend = {works:'规则成立',wrong:'状态失配',fixed:'修复后旧错误不可达或新增正确状态'};
