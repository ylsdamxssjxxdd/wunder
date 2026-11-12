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
