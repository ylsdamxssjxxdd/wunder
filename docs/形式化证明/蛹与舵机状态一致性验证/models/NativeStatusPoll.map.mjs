// AI生成
export const readerActions = ["SendA", "SendB", "SwitchToA", "SwitchToB", "Hide", "Restore"];

export function state(value) {
  const d = (t) => (value["d" + t] === "running" ? "运行中" : "已完成");
  const p = (t) => (value["p" + t] === "live" ? "运行中" : "已完成");
  const sel = value.active === "A" ? "A" : "B";
  const vis = value.observable ? "窗口可见" : "窗口隐藏";
  const fence = value.fB ? "；B 有残留本地标记" : "";
  return `实际 A ${d("A")}、B ${d("B")}；条目 A ${p("A")}、B ${p("B")}；选中 ${sel}；${vis}${fence}`;
}

export function step(action) {
  return {
    SendA: "向线程 A 发送一条消息",
    SendB: "向线程 B 发送一条消息",
    SwitchToA: "切换到线程 A",
    SwitchToB: "切换到线程 B",
    Hide: "窗口失去侧栏（隐藏，或被云端接管）",
    Restore: "窗口重新拥有侧栏",
    SettleA: "线程 A 在后台跑完（无本地操作）",
    SettleB: "线程 B 在后台跑完（无本地操作）",
    ObserveA: "A 的观察器把 A 的实际相位写入 A 的条目",
    ObserveB: "B 的观察器把 B 的实际相位写入 B 的条目",
    Poll: "轮询泵触发一次目录状态刷新",
  }[action] || action;
}

export function cause(value, facts, invariant) {
  return {
    BackgroundSettledIsShown:
      "线程 B 在后台结束，但上一轮本地留下的运行中标记仍在栅栏里，目录返回的已完成被拒绝，B 的条目停在运行中，直到读者切回 B",
  }[invariant];
}

export const legend = { works: "状态一致", wrong: "修复前风险", fixed: "修复后不可达" };