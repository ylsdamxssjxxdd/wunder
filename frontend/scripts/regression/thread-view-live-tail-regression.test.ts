// AI生成
// 云端版流式：tail 帧的偏移只用来定位，不用来证明连续性。
//
// 复现的现场：持久行只带被截断的窗口（正文头 2 KB、思考尾 2 KB），实时文本全靠
// `thread_item_tail` 追加；一旦服务端重建 emitter / 重试 / 重放，同一 item 的
// offset 会回到 0。旧实现拿“单调游标”判定重复帧，于是把新偏移小于旧游标的所有
// 帧全部丢掉——文字卡在开头，直到终态正文到达才一股脑出现。
import assert from 'node:assert/strict';
import {
  applyThreadViewChanges,
  applyThreadViewTail,
  beginThreadView,
  clearThreadView,
  commitThreadView,
  threadViewSlots
} from '../../src/realtime/chat/threadViewStore';

const key = 'fixture-stream-thread';
const row = (state: string, revision: number, content = '', reasoning = '', contentOffset = 0) => ({
  root_turn_id: 'fixture-turn',
  execution_turn_id: 'fixture-turn',
  user_round: 1,
  revision,
  status: state,
  user: { item_id: 'fixture-turn:user', content: 'fixture input' },
  assistant: {
    item_id: 'fixture-turn:text-1',
    content,
    reasoning,
    content_offset: contentOffset,
    reasoning_offset: 0,
    created_seq: 3
  },
  process_items: [
    { item_id: 'fixture-turn:tool-1', turn_id: 'fixture-turn', kind: 'tool_call', status: 'running', payload: { tool: 'exec_command', model_round: 1 } }
  ],
  workflow_count: 1,
  failed_count: 0
});
const page = (chat: unknown[], seq: number) => ({
  session_id: key,
  epoch: 1,
  view_seq: seq,
  projection_state: 'ready',
  thread_empty: false,
  latest_round: 1,
  chat,
  trajectory: [],
  tiles: [],
  tile_level: 0,
  next_before: null,
  has_more: false
});

const start = () => {
  clearThreadView(key);
  const generation = beginThreadView(key);
  assert.equal(commitThreadView(key, page([row('running', 3)], 3), generation), true);
};
const assistant = () => threadViewSlots(key)![0].assistant;
const tail = (field: 'content' | 'reasoning', offset: number, text: string) =>
  applyThreadViewTail(key, { item_id: 'fixture-turn:text-1', field, offset, text });

// 1) 正文：偏移从 0 正常推进。
start();
let offset = 0;
for (let index = 0; index < 40; index += 1) {
  const text = `正文${index}。`;
  tail('content', offset, text);
  offset += text.length;
}
assert.equal(String(assistant().__thread_live).length, offset, 'a contiguous stream accumulates every chunk');
assert.equal(String(assistant().content).length, offset, 'the bounded head keeps growing while it fits');

// 2) 服务端偏移空间重建（重试 / emitter 重建 / 重放）：必须继续追加，不能冻结。
let restarted = 0;
const beforeRestart = String(assistant().__thread_live).length;
for (let index = 0; index < 40; index += 1) {
  const text = `续写${index}。`;
  tail('content', restarted, text);
  restarted += text.length;
}
assert.equal(
  String(assistant().__thread_live).length,
  beforeRestart + restarted,
  'a restarted offset space keeps streaming instead of dropping every frame'
);

// 3) 同一帧重放只算一次（重连后的补发不能把文字翻倍）。
const replayText = '续写39。';
const replayOffset = Number(assistant().__thread_live_offset) - replayText.length;
const lengthBeforeReplay = String(assistant().__thread_live).length;
tail('content', replayOffset, replayText);
assert.equal(String(assistant().__thread_live).length, lengthBeforeReplay, 'an already-applied chunk is idempotent');

// 4) 思考：同样的规则，偏移重建后继续增长。
let reasoningOffset = 0;
for (let index = 0; index < 20; index += 1) {
  const text = `思考${index}。`;
  tail('reasoning', reasoningOffset, text);
  reasoningOffset += text.length;
}
assert.equal(String(assistant().reasoning).length, reasoningOffset);
let reasoningRestarted = 0;
const beforeReasoningRestart = String(assistant().reasoning).length;
for (let index = 0; index < 20; index += 1) {
  const text = `再想${index}。`;
  tail('reasoning', reasoningRestarted, text);
  reasoningRestarted += text.length;
}
assert.equal(
  String(assistant().reasoning).length,
  beforeReasoningRestart + reasoningRestarted,
  'a restarted reasoning offset space keeps the thought streaming'
);

// 5) 持久刷新只带来被截断的窗口：实时缓冲必须留着，正文才有连续文本可渲染。
const liveBeforeRefresh = String(assistant().__thread_live).length;
const head = String(assistant().__thread_live).slice(0, 64);
applyThreadViewChanges(key, {
  view_seq: 20,
  rows: [row('running', 20, head, '思考尾巴', liveBeforeRefresh)]
});
assert.equal(
  String(assistant().__thread_live).length,
  liveBeforeRefresh,
  'a durable row refresh never shortens the live answer'
);
assert.equal(String(assistant().content), head, 'the durable row only owns its bounded preview');

console.log('thread view stream: restarted offsets keep streaming, replays stay idempotent, live buffer survives durable refreshes pass');
