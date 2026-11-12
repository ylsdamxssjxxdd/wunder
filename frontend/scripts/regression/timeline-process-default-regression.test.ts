import test from 'node:test';
import assert from 'node:assert/strict';

const {
  isLatestProcessTurn,
  messageShowsWorkProcess,
  resolveLatestProcessTurnKey,
  timelineHasProcessFold,
  timelineShowsWorkProcess
} = require('../../src/components/chat/timelineProcessDefault') as typeof import('../../src/components/chat/timelineProcessDefault');

const body = (id: string, text = 'fixture body') => ({ kind: 'body', id, text });
const activity = (id: string) => ({ kind: 'activity', id, rows: [{ type: 'reasoning', itemId: `${id}:r` }] });

// 外壳只在「最后一段正文之前还有内容」时出现，判定与 MessageTimelineBlocks 的
// foldEnabled 同源：错一步就会出现「判成有过程却无处展开」。
test('wraps a process only when something precedes the final body', () => {
  assert.equal(timelineHasProcessFold([body('b1')]), false);
  assert.equal(timelineHasProcessFold([activity('a1'), body('b1')]), false);
  assert.equal(timelineHasProcessFold([activity('a1'), activity('a2'), body('b1')]), true);
  assert.equal(timelineHasProcessFold([body('b1'), activity('a1'), body('b2')]), true);
  assert.equal(timelineHasProcessFold([body('b1'), body('b2')]), true);
  assert.equal(timelineHasProcessFold([]), false);
  assert.equal(timelineHasProcessFold(null), false);
});

// 单批次轮次没有外壳，批次栏自己就是那行折叠，也算工作过程。
test('a lone tool batch still counts as a work process', () => {
  assert.equal(timelineShowsWorkProcess([activity('a1'), body('b1')]), true);
  assert.equal(timelineShowsWorkProcess([body('b1'), activity('a1')]), true);
  assert.equal(timelineShowsWorkProcess([body('b1')]), false);
  assert.equal(timelineShowsWorkProcess([]), false);
});

test('an empty batch row is not a process bar', () => {
  const emptyBatch = { kind: 'activity', id: 'a1', rows: [] };
  assert.equal(timelineShowsWorkProcess([emptyBatch, body('b1')]), false);
});

test('legacy rows expose a process through steps or reasoning', () => {
  assert.equal(messageShowsWorkProcess({ process_count: 3 }), true);
  assert.equal(messageShowsWorkProcess({ workflow_count: 1 }), true);
  assert.equal(messageShowsWorkProcess({ reasoning: 'fixture thought' }), true);
  assert.equal(messageShowsWorkProcess({ reasoning: '   ' }), false);
  assert.equal(messageShowsWorkProcess({ content: 'fixture answer' }), false);
  assert.equal(messageShowsWorkProcess(null), false);
});

test('projection blocks win over legacy counters when they exist', () => {
  // 有投影块时不走轻量展示窗分支：块里没有过程就没有过程条。
  assert.equal(messageShowsWorkProcess({ timeline: [body('b1')], process_count: 5 }), false);
  assert.equal(messageShowsWorkProcess({ timeline: [activity('a1'), body('b1')], process_count: 0 }), true);
});

test('picks the newest turn that actually carries a process', () => {
  const turns = [
    { key: 'turn-1', assistant: { message: { timeline: [activity('a1'), body('b1')] } } },
    { key: 'turn-2', assistant: { message: { timeline: [activity('a2'), body('b2')] } } },
    { key: 'turn-3', assistant: { message: { content: 'fixture answer' } } }
  ];
  assert.equal(resolveLatestProcessTurnKey(turns), 'turn-2');
  assert.equal(resolveLatestProcessTurnKey(turns.slice(0, 1)), 'turn-1');
  assert.equal(resolveLatestProcessTurnKey(turns.slice(-1)), '');
  assert.equal(resolveLatestProcessTurnKey([]), '');
  assert.equal(resolveLatestProcessTurnKey(undefined), '');
});

test('only the resolved key counts as the latest process', () => {
  assert.equal(isLatestProcessTurn('turn-2', 'turn-2'), true);
  assert.equal(isLatestProcessTurn('turn-1', 'turn-2'), false);
  assert.equal(isLatestProcessTurn('turn-1', ''), false);
  assert.equal(isLatestProcessTurn(undefined, undefined), false);
});
