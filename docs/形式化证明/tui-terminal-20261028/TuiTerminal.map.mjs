export const readerActions = ["InsertStream"];
export function state(value) {
  const prefix = `第 ${value.epoch} 轮：`;
  if (value.empty_output_error && value.reported_error) return prefix + "原始错误之后又出现空输出错误";
  if (value.empty_output_error) return prefix + "无输出时正常提示错误";
  if (value.phase === "streaming") return prefix + "新请求未继承旧错误";
  if (value.phase === "error") return prefix + "已显示原始错误";
  if (value.phase === "terminal") return prefix + (value.terminal_status === "failed" ? "取消终态保留原始错误" : "终态已到达");
  return prefix + "仅保留原始错误，等待下一轮";
}
export function step(action) {
  return ({ReceiveError: "收到并显示原始错误", RecordCancelled: "收到后续取消终态", RecordEmptyFinished: "无输出直接收到终态", Done: "流关闭并结算", InsertStream: "建立下一轮流并重置状态"})[action] || action;
}
export function group(value) {
  return value.empty_output_error && value.reported_error ? "重复报错" : "预期行为";
}
