import test from 'node:test';
import assert from 'node:assert/strict';

import { hasRetainedMessageConversationContext } from '../../src/views/messenger/messageConversationRetention';

test('retains message conversation context while current session is active even if mixed conversation list is transiently empty', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      activeSessionId: 'sess_current',
      messageCount: 2,
      worldConversationId: '',
      worldMessageCount: 0
    }),
    true
  );
});

test('retains message conversation context while route still points at the current chat even before active conversation is rehydrated', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      routeSessionId: 'sess_route_only',
      activeSessionId: '',
      draftAgentId: '',
      messageCount: 0
    }),
    true
  );
});

test('retains message conversation context while only local draft messages remain', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      draftAgentId: 'agent_current',
      messageCount: 1
    }),
    true
  );
});

test('allows empty state only when no route, session, draft, or message context remains', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      activeConversationId: '',
      routeConversationId: '',
      routeSessionId: '',
      routeAgentId: '',
      routeEntry: '',
      activeSessionId: '',
      draftAgentId: '',
      messageCount: 0,
      worldConversationId: '',
      worldMessageCount: 0
    }),
    false
  );
});
