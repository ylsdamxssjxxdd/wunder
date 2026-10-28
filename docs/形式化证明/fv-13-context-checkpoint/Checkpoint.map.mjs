export const readerActions = ['Stop', 'CrashAndReopen'];
export function state(values) {
  const phase = {generating:'生成摘要',ready:'摘要已就绪',checking:'检查提交权限',writing:'等待持久化',committed:'检查点已持久化',returned:'提交已返回',failed:'提交失败，保留原上下文',applied:'使用压缩上下文',reopened:'重开并恢复上下文'};
  return `${values.status === 'running' ? '轮次执行中' : '轮次已停止'}<br/>${phase[values.phase]}<br/>${values.durable ? '存在持久检查点' : '没有持久检查点'}${values.late ? '<br/>终态之后仍写入检查点' : ''}`;
}
export function step(action, before, after) {
  if (String(action).startsWith('AcquireCheckpoint') && after.gate === 'none') return '旧路径直接准备追加，未取得提交闸';
  if (String(action).startsWith('CheckRunning') && before.gate === 'none') return '旧路径未核对轮次状态';
  return ({SummaryReady:'摘要生成完成',AcquireCheckpoint:'取得线程提交闸',CheckRunning:'核对原轮次仍在执行',RejectCheckpoint:'拒绝迟到摘要',PersistCheckpoint:'持久提交检查点',WriteFails:'写入失败，放弃替换',ReleaseCheckpoint:'提交成功并释放闸门',ApplyReplacement:'应用压缩上下文',Stop:'停止并结清轮次',CrashAndReopen:'崩溃后从持久记录恢复'})[String(action).split('(')[0]] ?? String(action);
}
export function cause() { return '检查点追加绕过轮次提交闸，停止后仍可写入'; }
