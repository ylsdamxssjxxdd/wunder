import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveComposerContextUsageSource,
  resolveThreadCompacted
} from '../../src/components/chat/composerContextUsage';

// A compaction turns the composer's brain gauge red for the rest of the
// thread's life. The signal must be durable (it survives a reload) and must
// not fire for attempts that failed or were skipped.

test('a fresh thread is not compacted', () => {
  assert.equal(resolveThreadCompacted([], null), false);
  assert.equal(
    resolveThreadCompacted(
      [{ role: 'assistant', stats: { contextTokens: 1200 } }],
      { context_tokens: 1200 }
    ),
    false
  );
  assert.equal(resolveComposerContextUsageSource([], null, false).compacted, false);
});

test('a manual compaction marker keeps the thread compacted', () => {
  const messages = [
    { role: 'user', content: '/compact' },
    { role: 'assistant', manual_compaction_marker: true, content: '' }
  ];
  assert.equal(resolveThreadCompacted(messages, null), true);
  assert.equal(resolveComposerContextUsageSource(messages, null, false).compacted, true);
});

test('a finished compaction workflow item keeps the thread compacted', () => {
  const messages = [
    {
      role: 'assistant',
      stats: { contextTokens: 900 },
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          detail: JSON.stringify({ status: 'done', context_tokens_after: 0 })
        }
      ]
    }
  ];
  assert.equal(resolveThreadCompacted(messages, null), true);
});

test('a failed or skipped compaction does not mark the thread', () => {
  const messages = [
    {
      role: 'assistant',
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'failed',
          detail: JSON.stringify({ status: 'failed' })
        }
      ]
    },
    {
      role: 'assistant',
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          detail: JSON.stringify({ status: 'not_needed' })
        }
      ]
    }
  ];
  assert.equal(resolveThreadCompacted(messages, null), false);
});

test('a session flag from the backend is honoured', () => {
  assert.equal(resolveThreadCompacted([], { context_compacted: true }), true);
  assert.equal(resolveThreadCompacted([], { contextCompacted: '1' }), true);
  assert.equal(resolveThreadCompacted([], { context_compacted: false }), false);
});

test('the backend flag wins even when no compaction message is loaded', () => {
  // Paged history may start after the compaction, so the durable session flag
  // is the only signal left. The gauge must still paint red at any occupancy.
  const source = resolveComposerContextUsageSource(
    [{ role: 'assistant', stats: { contextTokens: 1200 } }],
    { context_tokens: 1200, context_total_tokens: 200000, context_compacted: true },
    false
  );
  assert.equal(source.compacted, true);
  assert.equal(source.contextTokens, 1200);
});

test('a durable compaction row marks the thread without a tool identity', () => {
  // The runtime persists the summary as its own `compaction` item, so there is
  // no toolName/toolCallId to match and no per-item status to read.
  const asWorkflowItem = [
    {
      role: 'assistant',
      workflowItems: [{ kind: 'compaction', content: 'history' }]
    }
  ];
  assert.equal(resolveThreadCompacted(asWorkflowItem, null), true);

  const asOwnMessage = [{ role: 'system', kind: 'compaction_summary' }];
  assert.equal(resolveThreadCompacted(asOwnMessage, null), true);

  const asPayload = [{ role: 'system', payload: { itemType: 'compacted' } }];
  assert.equal(resolveThreadCompacted(asPayload, null), true);
});

test('a durable compaction row marked failed is not counted', () => {
  const messages = [
    {
      role: 'assistant',
      workflowItems: [{ kind: 'compaction', status: 'failed' }]
    }
  ];
  assert.equal(resolveThreadCompacted(messages, null), false);
});
