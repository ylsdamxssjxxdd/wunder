export const readerActions = ['Submit', 'Replay', 'Stop', 'Crash', 'Restart'];
export function state(values) {
  const status = {waiting:'等待回答',completed:'问询已结清',cancelled:'问询已停止'};
  return `${status[values.status]}<br/>已受理回答：${values.answers.size ?? values.answers.length}<br/>消费身份：${values.claim}<br/>${values.online ? '进程在线' : '进程已崩溃'}${values.late ? '<br/>停止后仍受理新回答' : ''}`;
}
export function step(action) {
  return ({Submit:'客户端提交回答（受理事务）',Replay:'重试已受理回答',Settle:'结清原等待轮次',Stop:'停止问询',Crash:'进程在事务间崩溃',Restart:'重启并读取持久身份'})[String(action).split('(')[0]] ?? String(action);
}
export function cause(values, facts, rule) {
  return String(rule).includes('StopWins') ? '受理前未检查原轮次是否仍等待输入' : '只按客户端消息键去重，未绑定问询消费身份';
}
