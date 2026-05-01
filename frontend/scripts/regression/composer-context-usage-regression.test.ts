import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveComposerContextUsageSource } from '../../src/components/chat/composerContextUsage';

test('composer context usage prefers the running assistant over stale session max', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stats: {
          contextTokens: 27018
        }
      },
      {
        role: 'user',
        content: 'next'
      },
      {
        role: 'assistant',
        created_at: '2026-05-01T00:01:00.000Z',
        stream_incomplete: true,
        workflowStreaming: true,
        stats: {
          contextTokens: 25888
        }
      }
    ],
    {
      context_tokens: 27018
    },
    true
  );

  assert.equal(source.runningAssistant, true);
  assert.equal(source.contextTokens, 25888);
});

test('composer context usage merges session cache only after the assistant is stable', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stream_incomplete: false,
        workflowStreaming: false,
        stats: {
          contextTokens: 25888
        }
      }
    ],
    {
      context_tokens: 27018
    },
    false
  );

  assert.equal(source.runningAssistant, false);
  assert.equal(source.contextTokens, 27018);
});
