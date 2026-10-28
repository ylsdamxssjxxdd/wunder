export const readerActions = ["Cancel"];
export function state(value) {
  const scenario = ({auth:"鉴权失败",transient:"服务暂不可用",truncated:"流中断",tool:"工具失败后询问模型"})[value.scenario];
  if (value.permanentRetried) return `${scenario}：错误地重试同一鉴权请求`;
  if (value.bypassedBackoff) return `${scenario}：未退避又请求一次`;
  if (value.incompleteAccepted) return `${scenario}：不完整流被当作成功`;
  const phase = ({tool_ready:"准备执行工具",tool_finished:"工具返回失败",ready:"准备请求",calling:"请求在途",rejected:"收到拒绝",eof:"缺少完成标记",unavailable:"收到暂时故障",fallback_failed:"未退避又请求一次",backoff:"可取消的退避",completed:"返回成功",failed:"明确失败",cancelled:"取消结清",released:"已释放资源"})[value.phase];
  return `${scenario}：${value.cancelled ? "已停止，" : ""}${phase}`;
}
export function step(action) {
  return ({DispatchTool:"执行一次工具",ObserveToolFailure:"保留失败结果及调用身份",BeginCall:"通过取消闸后请求",ProviderFailure:"收到故障或不完整流",ProviderSuccess:"收到完整回复",LegacyFallback:"绕过退避切换非流式",ClassifyFailure:"分类故障并决定结清或重试",BackoffElapsed:"等待到期",Cancel:"取消等待分支被选中",CancelObserved:"取消错误向外结清",ReleaseResources:"结清并释放本代锁"})[action] || action;
}
export function group(value) {
  if (value.permanentRetried || value.bypassedBackoff || value.incompleteAccepted) return "错误处理缺口";
  return value.phase === "released" ? "资源已结清" : "正常恢复边界";
}
export function cause(value) {
  return value.scenario;
}
