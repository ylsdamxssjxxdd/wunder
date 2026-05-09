import test from 'node:test';
import assert from 'node:assert/strict';

import {
  hasRetainedAgentConversationContext,
  hasRetainedMessageConversationContext,
  resolveMessageConversationKind
} from '../../src/views/messenger/messageConversationRetention';

test('retains message conversation context while current session is active even if mixed conversation list is transiently empty', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      foregroundLock: false,
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
      foregroundLock: false,
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
      foregroundLock: false,
      draftAgentId: 'agent_current',
      messageCount: 1
    }),
    true
  );
});

test('retains agent conversation semantics while local message context is still present', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      foregroundLock: false,
      activeSessionId: '',
      draftAgentId: '',
      messageCount: 3
    }),
    true
  );
});

test('retains agent conversation semantics while send foreground lock is represented by draft or local context', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      foregroundLock: false,
      routeAgentId: 'agent_locked',
      activeSessionId: '',
      draftAgentId: '',
      messageCount: 1
    }),
    true
  );
});

test('retains message conversation context while send foreground lock is active even before session state settles', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      foregroundLock: true,
      activeSessionId: '',
      draftAgentId: '',
      messageCount: 0
    }),
    true
  );
});

test('retains agent conversation context while send foreground lock is active', () => {
  assert.equal(
    hasRetainedAgentConversationContext({
      foregroundLock: true,
      activeConversationKind: '',
      activeConversationId: '',
      messageCount: 0
    }),
    true
  );
});

test('does not retain agent conversation context for world conversations', () => {
  assert.equal(
    hasRetainedAgentConversationContext({
      foregroundLock: false,
      activeConversationKind: 'direct',
      activeConversationId: 'conv_world',
      worldConversationId: 'conv_world',
      worldMessageCount: 1
    }),
    false
  );
});

test('resolves message conversation kind to agent while foreground lock bridges session identity gaps', () => {
  assert.equal(
    resolveMessageConversationKind({
      foregroundLock: true,
      activeConversationKind: '',
      activeConversationId: '',
      activeSessionId: '',
      draftAgentId: '',
      messageCount: 0
    }),
    'agent'
  );
});

test('resolves message conversation kind to world for route-backed world conversations', () => {
  assert.equal(
    resolveMessageConversationKind({
      foregroundLock: false,
      routeConversationId: 'conv_world'
    }),
    'world'
  );
});

test('allows empty state only when no route, session, draft, or message context remains', () => {
  assert.equal(
    hasRetainedMessageConversationContext({
      foregroundLock: false,
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
