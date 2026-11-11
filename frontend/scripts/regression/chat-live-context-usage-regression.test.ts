// AI生成
import './mocks/browserStorage';
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  applyChatLiveContextUsageEvent,
  readChatLiveContextUsage,
  resetChatLiveContextUsage
} from '../../src/realtime/chat/chatLiveContextUsage';
import { applyChatThreadServerEvent } from '../../src/realtime/chat/chatThreadRuntime';
import { clearThreadView, commitThreadView } from '../../src/realtime/chat/threadViewStore';

const SESSION = 'sess-live-context';

const commitEmptyView = (): boolean =>
  commitThreadView(SESSION, {
    session_id: SESSION,
    epoch: 1,
    view_seq: 1,
    projection_state: 'ready',
    thread_empty: true,
    latest_round: 0,
    chat: [],
    trajectory: [],
    tiles: [],
    tile_level: 0,
    next_before: null,
    has_more: false
  });

test('committed thread views still capture live context usage frames', () => {
  resetChatLiveContextUsage();
  clearThreadView(SESSION);
  assert.equal(commitEmptyView(), true);

  const applied = applyChatThreadServerEvent(
    { activeSessionId: SESSION } as never,
    SESSION,
    'context_usage',
    {
      data: {
        context_tokens: 129521,
        context_occupancy_tokens: 129521,
        max_context: 260000,
        context_usage_source: 'provider_input',
        event_seq: 42
      }
    }
  );

  // The display window still owns the timeline and swallows the frame.
  assert.equal(applied, true);
  assert.deepEqual(readChatLiveContextUsage(SESSION), {
    used: 129521,
    total: 260000,
    seq: 42
  });
});

test('durable single-pane threads capture live context usage frames', () => {
  const session = 'sess-single-pane';
  resetChatLiveContextUsage();
  clearThreadView(session);
  applyChatThreadServerEvent(
    { activeSessionId: session } as never,
    session,
    'context_usage',
    {
      data: {
        context_occupancy_tokens: 6400,
        max_context: 128000,
        context_usage_source: 'provider_observed',
        event_seq: 7
      }
    }
  );

  assert.deepEqual(readChatLiveContextUsage(session), {
    used: 6400,
    total: 128000,
    seq: 7
  });
});

test('live context usage keeps the last observed value across unobserved resets', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent(SESSION, 'context_usage', {
    data: {
      context_tokens: 129521,
      context_occupancy_tokens: 129521,
      max_context: 260000,
      context_usage_source: 'provider_input',
      event_seq: 10
    }
  });
  applyChatLiveContextUsageEvent(SESSION, 'context_usage', {
    data: {
      context_tokens: 0,
      context_occupancy_tokens: 0,
      max_context: 260000,
      context_usage_source: 'unobserved_after_compaction',
      event_seq: 11
    }
  });

  assert.deepEqual(readChatLiveContextUsage(SESSION), {
    used: 129521,
    total: 260000,
    seq: 11
  });
});

test('live context usage ignores replayed frames with older sequence numbers', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent(SESSION, 'context_usage', {
    data: { context_occupancy_tokens: 129521, max_context: 260000, event_seq: 20 }
  });
  applyChatLiveContextUsageEvent(SESSION, 'context_usage', {
    data: { context_occupancy_tokens: 3000, max_context: 260000, event_seq: 19 }
  });

  assert.equal(readChatLiveContextUsage(SESSION)?.used, 129521);
  assert.equal(readChatLiveContextUsage(SESSION)?.seq, 20);
});

test('live context usage reads llm_output message stats and token usage input', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent(SESSION, 'llm_output', {
    data: {
      message_stats: {
        contextTokens: 140000,
        context_occupancy_tokens: 140000
      },
      event_seq: 30
    }
  });
  assert.equal(readChatLiveContextUsage(SESSION)?.used, 140000);

  applyChatLiveContextUsageEvent(SESSION, 'token_usage', {
    data: {
      usage: { input_tokens: 142000, output_tokens: 40, estimated: false },
      event_seq: 31
    }
  });
  assert.equal(readChatLiveContextUsage(SESSION)?.used, 142000);
});

test('live context usage applies completed compaction after-tokens', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent(SESSION, 'context_usage', {
    data: { context_occupancy_tokens: 129521, max_context: 260000, event_seq: 40 }
  });
  applyChatLiveContextUsageEvent(SESSION, 'compaction', {
    data: {
      status: 'done',
      context_tokens_after: 18000,
      observed_context_tokens_after: 18000,
      max_context: 260000,
      event_seq: 41
    }
  });
  assert.equal(readChatLiveContextUsage(SESSION)?.used, 18000);

  applyChatLiveContextUsageEvent(SESSION, 'compaction', {
    data: { status: 'failed', context_tokens_after: 1, event_seq: 42 }
  });
  assert.equal(readChatLiveContextUsage(SESSION)?.used, 18000);
});

test('live context usage stays unknown until the first observation', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent('sess-fresh', 'context_usage', {
    data: {
      context_tokens: 0,
      context_occupancy_tokens: 0,
      max_context: 128000,
      context_usage_source: 'unobserved'
    }
  });

  assert.equal(readChatLiveContextUsage('sess-fresh'), null);
});

test('live context usage is isolated per session', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent('sess-a', 'context_usage', {
    data: { context_occupancy_tokens: 3200, max_context: 128000 }
  });
  applyChatLiveContextUsageEvent('sess-b', 'context_usage', {
    data: { context_occupancy_tokens: 6400, max_context: 128000 }
  });

  assert.equal(readChatLiveContextUsage('sess-a')?.used, 3200);
  assert.equal(readChatLiveContextUsage('sess-b')?.used, 6400);
  assert.equal(readChatLiveContextUsage('sess-missing'), null);
});
