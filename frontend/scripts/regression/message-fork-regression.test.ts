import test from 'node:test';
import assert from 'node:assert/strict';

const {
  canForkAgentMessage,
  resolveAgentMessageForkRound
} = require('../../src/views/messenger/messageFork') as typeof import('../../src/views/messenger/messageFork');

test('resolves the durable user round from thread-view and runtime messages', () => {
  assert.equal(resolveAgentMessageForkRound({ user_round: 3 }), 3);
  assert.equal(resolveAgentMessageForkRound({ userRound: 4 }), 4);
  assert.equal(resolveAgentMessageForkRound({ user_turn_index: 5 }), 5);
  assert.equal(resolveAgentMessageForkRound({ __runtime_user_round: 6 }), 6);
  assert.equal(resolveAgentMessageForkRound({ user_round: 0, __runtime_user_round: 2 }), 2);
  assert.equal(resolveAgentMessageForkRound({ user_round: -1 }), null);
  assert.equal(resolveAgentMessageForkRound({ user_round: 1.5 }), null);
  assert.equal(resolveAgentMessageForkRound({}), null);
});

test('only settled assistant messages with a durable round can be forked', () => {
  assert.equal(canForkAgentMessage({ role: 'assistant', user_round: 1, status: 'completed' }), true);
  assert.equal(canForkAgentMessage({ role: 'assistant', user_round: 1, status: 'failed' }), true);
  assert.equal(canForkAgentMessage({ role: 'user', user_round: 1 }), false);
  assert.equal(canForkAgentMessage({ role: 'assistant' }), false);
  assert.equal(canForkAgentMessage({ role: 'assistant', user_round: 1, status: 'running' }), false);
  for (const flag of ['streaming', 'stream_incomplete', 'workflowStreaming', 'reasoningStreaming']) {
    assert.equal(canForkAgentMessage({ role: 'assistant', user_round: 1, [flag]: true }), false);
  }
});
