export const readerActions = ['TerminalHistory'];
export function state(values) {
  const states = {running:'气泡运行',completed:'气泡完成'};
  const tools = {running:'旧工具转圈',completed:'工具结果已更新',unloaded:'工具页待重读'};
  return `${states[values.status]}，${tools[values.entries]}<br/>${values.paged ? '已有过程页' : '尚未加载过程页'}`;
}
export function step(action) { return {TerminalHistory:'完成历史替换运行历史',RefreshTimer:'可见过程重新读取最新页'}[action] ?? action; }
export function cause() { return '终态保留旧过程页，运行刷新定时器却已经停止'; }
export const legend = {works:'规则成立',wrong:'状态失配',fixed:'修复后旧错误不可达或新增正确状态'};
