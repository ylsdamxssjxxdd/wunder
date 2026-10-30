// 读者：排查“目标不继续驱动”的工程师。
// 修复前的模型没有 deferred 变量，标签先判断它是否存在于状态里。
export const readerActions = ["CreateGoalDuringTurn"];
export function state(v) {
  const deferred = "deferred" in v ? v.deferred : false;
  if (v.goal === "none") return v.busy ? "No goal, human turn owns the session" : "No goal, session free";
  if (v.busy) return deferred ? "Goal waits for the human turn, wake kept for the release" : "Goal waits for the human turn";
  const done = v.rounds === 1 ? "1 round done" : `${v.rounds} rounds done`;
  if (v.inflight) return `Round ${v.rounds} is running`;
  if (v.requested) return `Next round requested, ${done}`;
  return `Active goal, ${done}, nothing next`;
}
export function step(action) {
  return {
    CreateGoalDuringTurn: "Model creates the goal mid-turn",
    DriveWhileBusy: "Driver wake meets the busy session",
    DriveWhenIdle: "Driver starts the next goal round",
    RoundEnds: "Goal round ends",
    LeaseRelease: "Human turn ends, session fence released",
  }[action] || action;
}
