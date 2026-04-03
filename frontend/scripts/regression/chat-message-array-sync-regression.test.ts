import test from 'node:test';
import assert from 'node:assert/strict';

import { replaceMessageArrayKeepingReference } from '../../src/stores/chatMessageArraySync';

test('replaceMessageArrayKeepingReference preserves the original array object', () => {
  const target = [{ role: 'user', content: 'old' }];
  const next = [
    { role: 'user', content: 'new user' },
    { role: 'assistant', content: 'new reply' }
  ];

  const result = replaceMessageArrayKeepingReference(target, next);

  assert.equal(result, target);
  assert.deepEqual(target, next);
});

test('replaceMessageArrayKeepingReference returns the next array when no target exists', () => {
  const next = [{ role: 'assistant', content: 'reply' }];
  const result = replaceMessageArrayKeepingReference(null, next);

  assert.equal(result, next);
});
