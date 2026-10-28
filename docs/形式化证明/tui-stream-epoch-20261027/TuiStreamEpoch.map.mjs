export const readerActions = ["Receive", "StartNextTurn"];
const turn = value => value[1];
const id = value => value[2];
export function state(v) {
  const missing = v.expected.filter(item => !v.output.some(shown => turn(item) === turn(shown) && id(item) === id(shown)));
  return missing.length ? `第 ${turn(missing[0])} 轮有事件未显示` : `第 ${v.epoch} 轮已显示全部到达事件`;
}
export function step(action) { return action.startsWith("Receive") ? "收到并投影一条流事件" : "开始下一轮并重置流去重窗口"; }
export function cause() { return "传输事件编号在每轮重新开始，旧实现却把去重窗口保留到线程结束"; }
