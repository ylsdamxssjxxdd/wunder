export const readerActions = ["Start", "Advance", "Finish", "Exhaust"];

export function state(value) {
  const phase = {
    idle: "等待打开较早轮次",
    scan: "读取 canonical 记录",
    done: "轮次条目已返回",
    truncated: "预算耗尽，页面为空",
  }[value.phase]
    || value.phase;
  const found = value.collected.length
    ? `已收集 ${value.collected.length} 项`
    : "尚未返回条目";
  return `${phase}，${found}`;
}

export function step(action) {
  return {
    Start: "打开活动窗口外的轮次",
    Advance: "逐条读取 rollout 记录",
    Finish: "按新到旧返回轮次条目",
    Exhaust: "记录预算在轮次前耗尽",
  }[action] || action;
}

export function cause(value, facts, invariant) {
  return {
    TrimmedTurnNeverEmpty: "从稀疏锚点起步，预算在到达轮次条目之前耗尽",
  }[invariant];
}

export const legend = { works: "正常回读", wrong: "修复前风险", fixed: "修复后不可达" };
