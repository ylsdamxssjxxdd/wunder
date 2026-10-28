export const readerActions = ["VerifyOutcome"];
export function state(value) {
  if (value.sideEffects === 2) return "未核对结果，副作用重复发生";
  if (value.phase === "observation") return value.retryable ? "工具超时，却建议可直接重试" : "工具超时，明确结果未知并要求核对";
  if (value.phase === "verified") return "已先核对执行结果";
  return value.sideEffects ? "命令已写入，尚未返回" : "命令执行中";
}
export function step(action) {
  return ({SideEffect:"外部命令先完成一次写入",Timeout:"等待超时并返回错误建议",ConsumeRetryAdvice:"调用者按建议重试",VerifyOutcome:"先检查目标状态"})[action] || action;
}
export function group(value) { return value.retryable && value.unknown ? "危险重试建议" : "保守恢复契约"; }
