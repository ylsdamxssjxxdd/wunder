// AI生成
export const readerActions = ["Write1", "Write2", "Send", "Shrink1", "Shrink2", "DriftSystem"];

export function state(value) {
  const written = value.written.length ? `已写入 ${value.written.length} 条条目` : "上下文为空";
  const sent = value.requests.length ? `已发送 ${value.requests.length} 次请求` : "尚未发送请求";
  const sys = value.sys === "S0" ? "system 块已冻结" : "system 块已漂移";
  return `${written}，${sent}，${sys}`;
}

export function step(action) {
  return {
    Write1: "追加 1 号持久条目",
    Write2: "追加 2 号持久条目",
    Send: "发送一次模型请求（前缀=已写入条目的回放投影）",
    Shrink1: "在两次请求之间回改 1 号条目的回放投影",
    Shrink2: "在两次请求之间回改 2 号条目的回放投影",
    DriftSystem: "在首轮之后重建 system 块",
  }[action] || action;
}

export function cause(value, facts, invariant) {
  return {
    AppendOnlyPrefix: "已发送过的条目被重新投影，新请求不再重复旧请求的消息前缀",
    SystemStable: "system 块在首轮之后被重建，raw prompt 前缀随之改变",
  }[invariant];
}

export const legend = { works: "前缀保持", wrong: "修复前风险", fixed: "修复后不可达" };