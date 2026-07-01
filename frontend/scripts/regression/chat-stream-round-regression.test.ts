import test from 'node:test';
import assert from 'node:assert/strict';

import {
  parseSegmentedDelta,
  resolveEventModelRoundNumber,
  resolveEventUserRoundNumber
} from '../../src/stores/chatStreamIds';

test('stream ids do not treat model round as user round', () => {
  const payload = {
    data: {
      data: {
        user_round: 1,
        model_round: 4,
        content: 'answer'
      },
      session_id: 'session-id'
    },
    event: 'llm_output'
  };

  assert.equal(resolveEventUserRoundNumber(payload, payload.data), 1);
  assert.equal(resolveEventModelRoundNumber(payload, payload.data), 4);
});

test('stream ids ignore bare model round when resolving user round', () => {
  const payload = {
    model_round: 4,
    content: 'answer'
  };

  assert.equal(resolveEventUserRoundNumber(payload, payload), null);
  assert.equal(resolveEventModelRoundNumber(payload, payload), 4);
});

test('stream ids keep segmented user round and model round separate', () => {
  const parsed = parseSegmentedDelta(
    {
      segments: [
        { delta: 'a', user_round: 1, model_round: 4 },
        { delta: 'b', model_round: 5 }
      ]
    },
    null
  );

  assert.equal(parsed?.delta, 'ab');
  assert.equal(parsed?.userRound, 1);
  assert.equal(parsed?.modelRound, 5);
  assert.equal(parsed?.round, 1);
});

