export const readerActions = [];
export function state(value) {
  const title = { prefix: '相同前缀的新思考', tail: '迟到的旧轮次增量', snapshot: '迟到的持久快照' }[value.scenario];
  const phase = value.phase === 0 ? '第一轮已完成' : value.phase === 1 ? '第二轮正在思考' : '迟到数据处理结束';
  return `${title}<br/>${phase}<br/>活动第${value.liveRound}轮，展示第${value.visibleRound}轮<br/>旧思考${value.oldText === 'Fixture repeated completed' ? '保持完整' : '被新文字覆盖'}`;
}
export function step(action) { return { NextRound: '下一轮开始输出', BindReasoning: '按条目身份匹配思考', LateTail: '收到旧轮次增量', DurableRefresh: '收到较旧模型条目的快照' }[action] || action; }
export function group(value) { return value.phase === 2 ? '处理结束' : '流式处理'; }
export function cause(value) { return value.scenario; }
