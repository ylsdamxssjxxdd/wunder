export const readerActions = [];
export function state(value) {
  const scenario = ({body_error:"成功状态携带错误",event_auth:"永久拒绝含误导正文",null_success:"正常回复含空错误字段",split_utf8:"中文跨网络分块",malformed:"无效编码或损坏事件"})[value.scenario];
  const phase = ({received:"收到响应",decoded:"读取分块完成",failure:"识别为故障",ready:"准备返回结果",settled:"请求已结清"})[value.phase];
  const outcome = ({none:"",success:value.corrupted ? "，文字或参数已损坏" : "，返回成功",permanent:"，明确永久拒绝",context:"，错误触发上下文恢复",transient:"，交给有界退避",other:"，错误拒绝正常回复"})[value.decision];
  return `${scenario}：${phase}${outcome}`;
}
export function step(action) {
  return ({Decode:"按网络分块读取字节",Validate:"验证编码和错误封装",Classify:"错误码优先分类",Accept:"返回正文与工具提议"})[action] || action;
}
export function cause(value) {
  return ({body_error:"错误封装被忽略",event_auth:"正文覆盖永久错误码",null_success:"空字段被当作故障",split_utf8:"逐块替换了半个字符",malformed:"损坏内容被静默跳过"})[value.scenario];
}
export function group(value) {
  return value.phase === "settled" ? "响应已结清" : "协议验证中";
}
