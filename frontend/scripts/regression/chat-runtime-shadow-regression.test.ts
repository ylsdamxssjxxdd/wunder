import test from 'node:test';
import assert from 'node:assert/strict';

import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { compareChatRuntimeShadow } from '../../src/realtime/chat/chatRuntimeShadow';
import type { ChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeTypes';

const apply = (events: ChatRuntimeEvent[]) => {
  const projection = createChatRuntimeProjection();
  events.forEach((event) => applyChatRuntimeEvent(projection, event));
  return projection;
};

test('chat runtime shadow accepts matching projection and legacy messages', () => {
  const user = {
    message_id: 'message-user-1',
    role: 'user',
    content: 'hello',
    created_at: '2026-04-30T02:14:06.000Z'
  };
  const assistant = {
    message_id: 'message-assistant-1',
    role: 'assistant',
    content: 'hi',
    reasoning: 'short',
    created_at: '2026-04-30T02:14:07.000Z'
  };
  const projection = apply([
    {
      event_type: 'session_snapshot',
      source: 'snapshot',
      strict: false,
      session_id: 'session-1',
      messages: [user, assistant],
      loading: false,
      running: false
    }
  ]);

  const report = compareChatRuntimeShadow({
    projection,
    sessionId: 'session-1',
    legacyMessages: [user, assistant],
    legacyBusy: false,
    phase: 'test'
  });

  assert.equal(report.ok, true);
  assert.equal(report.issues.length, 0);
  assert.equal(report.matchedCount, 2);
});

test('chat runtime shadow reports legacy duplicate messages by stable id', () => {
  const projection = apply([
    {
      event_type: 'user_message_created',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      message_id: 'message-user-1',
      content: 'hello'
    }
  ]);
  const duplicate = {
    message_id: 'message-user-1',
    role: 'user',
    content: 'hello'
  };

  const report = compareChatRuntimeShadow({
    projection,
    sessionId: 'session-1',
    legacyMessages: [duplicate, { ...duplicate }],
    legacyBusy: true
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'legacy_duplicate_message'));
});

test('chat runtime shadow reports missing messages on both sides', () => {
  const projection = apply([
    {
      event_type: 'user_message_created',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      message_id: 'message-user-1',
      content: 'hello'
    }
  ]);

  const report = compareChatRuntimeShadow({
    projection,
    sessionId: 'session-1',
    legacyMessages: [
      {
        message_id: 'message-user-2',
        role: 'user',
        content: 'other'
      }
    ],
    legacyBusy: true
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'legacy_missing_projected_message'));
  assert.ok(report.issues.some((issue) => issue.code === 'projection_missing_legacy_message'));
});

test('chat runtime shadow reports order drift for matched messages', () => {
  const user = {
    message_id: 'message-user-1',
    role: 'user',
    content: 'hello'
  };
  const assistant = {
    message_id: 'message-assistant-1',
    role: 'assistant',
    content: 'hi',
    user_turn_id: 'turn-1',
    model_turn_id: 'model-turn-1'
  };
  const projection = apply([
    {
      event_type: 'session_snapshot',
      source: 'snapshot',
      strict: false,
      session_id: 'session-1',
      messages: [user, assistant],
      loading: false,
      running: false
    }
  ]);

  const report = compareChatRuntimeShadow({
    projection,
    sessionId: 'session-1',
    legacyMessages: [assistant, user],
    legacyBusy: false
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'message_order_drift'));
});

test('chat runtime shadow reports content, reasoning, status and busy drift', () => {
  const projection = apply([
    {
      event_type: 'assistant_delta',
      source: 'test',
      strict: true,
      session_id: 'session-1',
      event_id: 'event-1',
      event_seq: 1,
      user_turn_id: 'turn-1',
      model_turn_id: 'model-turn-1',
      message_id: 'message-assistant-1',
      delta: 'live',
      reasoning_delta: 'thinking'
    }
  ]);

  const report = compareChatRuntimeShadow({
    projection,
    sessionId: 'session-1',
    legacyMessages: [
      {
        message_id: 'message-assistant-1',
        role: 'assistant',
        content: 'stale',
        reasoning: '',
        stream_incomplete: false
      }
    ],
    legacyBusy: false
  });

  assert.equal(report.ok, false);
  assert.ok(report.issues.some((issue) => issue.code === 'message_content_drift'));
  assert.ok(report.issues.some((issue) => issue.code === 'message_reasoning_drift'));
  assert.ok(report.issues.some((issue) => issue.code === 'message_status_drift'));
  assert.ok(report.issues.some((issue) => issue.code === 'busy_state_drift'));
});
