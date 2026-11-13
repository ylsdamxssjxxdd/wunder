import test from 'node:test';
import assert from 'node:assert/strict';

import { collectProcessSlots, isCompactionItem, mapWorkflowItem } from '../../src/components/chat/turnProcessRows';
import { buildTimelineToolEntry } from '../../src/components/chat/toolTimelineModel';
import { buildWorkflowToolRuns } from '../../src/components/chat/toolWorkflowRunModel';

/** 参数化翻译：断言只看结构，不绑定具体文案。 */
const t = (key: string) => key;

/**
 * 运行时真实落库的自动压缩条目（取自线程日志：kind=compaction，
 * payload 里只有 summary_text / content=history，没有 tool 字段）。
 */
const durableCompactionItem = (overrides: Record<string, unknown> = {}) => ({
  session_id: 'fixture-session',
  turn_id: 'fixture-turn',
  root_turn_id: 'fixture-turn',
  item_id: 'fixture-turn:compaction-cmp_fixture',
  item_index: 42,
  created_seq: 43,
  revision: 1,
  kind: 'compaction',
  status: 'done',
  visibility: 'user',
  payload: {
    _content_bytes: 7,
    _content_utf16: 7,
    content: 'history',
    event_type: 'compaction',
    process_seq: 2,
    status: 'done',
    summary_text: '[上下文摘要]\n本轮处理已完成，共执行 1 个模型轮次。',
    compaction_id: 'cmp_fixture',
    model_round: 3
  },
  created_time: 1,
  updated_time: 1,
  ...overrides
});

test('durable compaction item stays in the work process instead of being filtered out', () => {
  const items = [
    {
      item_id: 'fixture-turn:text-3',
      turn_id: 'fixture-turn',
      kind: 'assistant_message',
      status: 'completed',
      model_round: 3,
      payload: { content: 'fixture answer', reasoning: 'fixture thought', model_round: 3 }
    },
    durableCompactionItem()
  ];
  const { slots, mappedItems } = collectProcessSlots(items);
  assert.deepEqual(slots.map(slot => slot.type), ['message', 'tool']);
  assert.equal(mappedItems.length, 1);
  const mapped = mappedItems[0];
  assert.equal(mapped.eventType, 'compaction');
  assert.equal(mapped.toolName, 'context_compaction');
  assert.equal(mapped.model_round, 3);
  // 摘要必须来自 summary_text，不能被 content 里的原因标记覆盖。
  const detail = JSON.parse(String(mapped.detail));
  assert.match(String(detail.summary_text), /上下文摘要/);
});

test('compaction entry renders the compaction card through the shared tool entry chain', () => {
  const mapped = mapWorkflowItem(durableCompactionItem(), new Map())!;
  const runs = buildWorkflowToolRuns([mapped]);
  assert.equal(runs.length, 1);
  const entry = buildTimelineToolEntry(runs[0], t);
  assert.equal(entry.kind, 'compaction');
  assert.equal(entry.toolLabel, 'chat.toolWorkflow.compaction.title');
  assert.ok(entry.summary.length > 0, 'collapsed row needs a summary');
  assert.ok(entry.compaction, 'expanded body needs the compaction view');
});

test('legacy marked system message compaction keeps its summary and identity', () => {
  const item = {
    item_id: 'fixture-turn:system-compaction',
    turn_id: 'fixture-turn',
    kind: 'system_message',
    status: 'completed',
    payload: {
      content: 'Retained compacted history.',
      meta: { type: 'compaction_summary', compaction_id: 'cmp_system' }
    }
  };
  const { mappedItems } = collectProcessSlots([item]);
  assert.equal(mappedItems.length, 1);
  assert.equal(mappedItems[0].toolName, 'context_compaction');
  assert.match(String(mappedItems[0].detail), /Retained compacted history/);
});

test('ordinary system prompts and identity-less lifecycle rows stay out of the process list', () => {
  const items = [
    { item_id: 'fixture-turn:system', turn_id: 'fixture-turn', kind: 'system_message', status: 'completed', payload: { content: 'internal prompt' } },
    { item_id: 'fixture-turn:tool-lifecycle', turn_id: 'fixture-turn', kind: 'tool_call', status: 'running', payload: {} }
  ];
  const { slots, mappedItems } = collectProcessSlots(items as Array<Record<string, unknown>>);
  assert.equal(slots.length, 0);
  assert.equal(mappedItems.length, 0);
});

test('compaction kind detection covers the durable kinds and ignores reason-only content', () => {
  assert.equal(isCompactionItem(durableCompactionItem()), true);
  assert.equal(isCompactionItem({ kind: 'compacted', payload: {} }), true);
  const withoutSummary = mapWorkflowItem(
    durableCompactionItem({ payload: { content: 'history', event_type: 'compaction', status: 'done' } }),
    new Map()
  )!;
  assert.equal(withoutSummary.toolName, 'context_compaction');
  assert.equal(JSON.parse(String(withoutSummary.detail)).summary_text, undefined);
});
