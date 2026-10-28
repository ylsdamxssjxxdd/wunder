export const readerActions = ["EnterCommand"];
export function state(value) {
  const action = { create: "Create goal", save: "Save changed objective", pause: "Pause goal" }[value.command];
  if (value.stage !== "done") {
    const stage = { ready: "ready", controlled: "control acquired", fenced: "new rounds fenced", cancelled: "previous executor fenced", persisted: "command persisted", recorded: "command acknowledged" }[value.stage];
    return `${action}: ${stage}`;
  }
  if (value.command === "create") return value.durableCommand ? "Active goal, command survives reload" : "Active goal, command missing after reload";
  if (value.command === "save") return value.objective === "revised" ? "Changed objective saved" : "Save returned, original objective retained";
  return value.running ? "Paused banner, executor still running" : "Paused goal, executor fenced";
}
export function step(action) {
  return { EnterCommand: "User submits the command", DisarmAdmission: "Fence continuation admission", CancelExecutor: "Cancel and settle previous execution", PersistCommand: "Accept durable user turn", CompleteCommand: "Complete control turn", CreateGoal: "Goal creation returns", SaveGoal: "Banner save returns", PauseGoal: "Pause command returns" }[action] || action;
}
