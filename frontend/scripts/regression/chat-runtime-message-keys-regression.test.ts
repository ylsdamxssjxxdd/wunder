import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveChatRuntimeRenderableKey,
  resolveStableChatRuntimeMessageId
} from '../../src/realtime/chat/chatRuntimeMessageKeys';
import { resolveChatRuntimeMessageRenderKey } from '../../src/realtime/chat/chatRuntimeRenderAdapter';

test('chat runtime message keys align legacy raw and projection render keys by stable message id', () => {
  const legacy = {
    message_id: 'message-1',
    role: 'assistant',
    content: 'reply'
  };
  const projection = {
    __runtime_message_id: 'message-1',
    role: 'assistant',
    content: 'reply'
  };

  assert.equal(resolveChatRuntimeRenderableKey(legacy, 7), 'runtime:assistant:message-1');
  assert.equal(resolveChatRuntimeMessageRenderKey(projection), 'runtime:assistant:message-1');
});

test('chat runtime message keys prefer explicit runtime render key when present', () => {
  assert.equal(
    resolveChatRuntimeRenderableKey({
      __runtime_render_key: 'runtime:assistant:projected-message',
      message_id: 'legacy-message',
      role: 'assistant'
    }, 3),
    'runtime:assistant:projected-message'
  );
});

test('chat runtime message keys use client and request identities before index fallback', () => {
  assert.equal(
    resolveStableChatRuntimeMessageId({
      client_message_id: 'client-message-1',
      request_id: 'request-1',
      role: 'user'
    }),
    'client-message-1'
  );
  assert.equal(
    resolveChatRuntimeRenderableKey({
      request_id: 'request-1',
      role: 'assistant'
    }, 4),
    'runtime:assistant:request-1'
  );
});

test('chat runtime message keys keep index fallback only for messages without stable identity', () => {
  assert.equal(
    resolveChatRuntimeRenderableKey({
      role: 'assistant',
      content: 'legacy placeholder'
    }, 6),
    'legacy:assistant:6'
  );
});
