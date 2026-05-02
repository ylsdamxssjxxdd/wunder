import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveComposerContextUsageSource,
  resolveComposerRunningContextDisplayState
} from '../../src/components/chat/composerContextUsage';

test('composer context usage exposes running assistant raw value before display stabilization', () => {
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
  assert.equal(source.runningContextTokens, 25888);
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

test('composer context usage keeps completed assistant final value ahead of stale session estimate', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stream_incomplete: false,
        workflowStreaming: false,
        stats: {
          usage: {
            input_tokens: 25725,
            output_tokens: 85,
            total_tokens: 25810
          }
        }
      }
    ],
    {
      context_tokens: 21024
    },
    false
  );

  assert.equal(source.runningAssistant, false);
  assert.equal(source.contextTokens, 25810);
});

test('composer context usage lets the display layer stabilize a new round estimate', () => {
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
          contextTokens: 1534
        }
      }
    ],
    {
      context_tokens: 27018
    },
    true
  );

  assert.equal(source.runningAssistant, true);
  assert.equal(source.contextTokens, 1534);
  assert.equal(source.runningContextTokens, 1534);
});

test('composer context usage grows from the previous confirmed value during streaming', () => {
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
          contextTokens: 27240
        }
      }
    ],
    {
      context_tokens: 27018
    },
    true
  );

  assert.equal(source.runningAssistant, true);
  assert.equal(source.contextTokens, 27240);
});

test('composer context usage exposes current round raw tokens while running', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stats: {
          contextTokens: 25763
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
          contextTokens: 2883
        }
      }
    ],
    {
      context_tokens: 25763
    },
    true
  );

  assert.equal(source.runningAssistant, true);
  assert.equal(source.contextTokens, 2883);
  assert.equal(source.runningContextTokens, 2883);
});

test('composer context usage prefers live context over stale usage while running', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stats: {
          usage: {
            input_tokens: 25000,
            output_tokens: 763,
            total_tokens: 25763
          },
          contextTokens: 25763
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
          usage: {
            input_tokens: 25000,
            output_tokens: 855,
            total_tokens: 25855
          },
          contextTokens: 2883
        }
      }
    ],
    {
      context_tokens: 25763
    },
    true
  );

  assert.equal(source.runningAssistant, true);
  assert.equal(source.contextTokens, 2883);
  assert.equal(source.runningContextTokens, 2883);
});

test('composer context usage ignores model usage totals while running without explicit context', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stats: {
          contextTokens: 26716
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
          usage: {
            input_tokens: 26728,
            output_tokens: 949,
            total_tokens: 27677
          }
        }
      }
    ],
    {
      context_tokens: 26716
    },
    true
  );

  assert.equal(source.runningAssistant, true);
  assert.equal(source.contextTokens, 26716);
  assert.equal(source.runningContextTokens, null);
});

test('composer context usage aligns completed assistant with final usage totals', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stream_incomplete: false,
        workflowStreaming: false,
        stats: {
          usage: {
            input_tokens: 26728,
            output_tokens: 949,
            total_tokens: 27677
          }
        }
      }
    ],
    {
      context_tokens: 3504
    },
    false
  );

  assert.equal(source.runningAssistant, false);
  assert.equal(source.contextTokens, 27677);
});

test('composer context usage replaces completed request estimates with final bubble usage', () => {
  const source = resolveComposerContextUsageSource(
    [
      {
        role: 'assistant',
        created_at: '2026-05-01T00:00:00.000Z',
        stream_incomplete: false,
        workflowStreaming: false,
        stats: {
          contextTokens: 21024,
          usage: {
            input_tokens: 25725,
            output_tokens: 85,
            total_tokens: 25810
          }
        }
      }
    ],
    {
      context_tokens: 21024
    },
    false
  );

  assert.equal(source.runningAssistant, false);
  assert.equal(source.contextTokens, 25810);
});

test('composer context usage keeps post-tool raw context resets visually monotonic', () => {
  let state = resolveComposerRunningContextDisplayState({
    stableTokens: 26716,
    baseTokens: 26716,
    rawBaseTokens: 3460,
    lastRawTokens: 3460,
    runningRawTokens: 3504
  });

  assert.equal(state.stableTokens, 26760);

  state = resolveComposerRunningContextDisplayState({
    stableTokens: state.stableTokens,
    baseTokens: state.baseTokens,
    rawBaseTokens: state.rawBaseTokens,
    lastRawTokens: state.lastRawTokens,
    runningRawTokens: 3847
  });

  assert.equal(state.stableTokens, 27103);

  state = resolveComposerRunningContextDisplayState({
    stableTokens: state.stableTokens,
    baseTokens: state.baseTokens,
    rawBaseTokens: state.rawBaseTokens,
    lastRawTokens: state.lastRawTokens,
    runningRawTokens: 3578
  });

  assert.equal(state.stableTokens, 27103);
  assert.equal(state.baseTokens, 27103);
  assert.equal(state.rawBaseTokens, 3578);

  state = resolveComposerRunningContextDisplayState({
    stableTokens: state.stableTokens,
    baseTokens: state.baseTokens,
    rawBaseTokens: state.rawBaseTokens,
    lastRawTokens: state.lastRawTokens,
    runningRawTokens: 3600
  });

  assert.equal(state.stableTokens, 27125);
});
