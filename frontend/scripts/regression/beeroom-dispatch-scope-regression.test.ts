import test from 'node:test';
import assert from 'node:assert/strict';

import { isBeeroomDispatchScopeCurrent } from '../../src/components/beeroom/beeroomDispatchSessionPolicy';

test('late dispatch events cannot cross a group or session boundary', () => {
  assert.equal(
    isBeeroomDispatchScopeCurrent({
      expectedGroupId: 'group-a',
      currentGroupId: 'group-a',
      expectedSessionId: 'session-a',
      currentSessionId: 'session-a'
    }),
    true
  );
  assert.equal(
    isBeeroomDispatchScopeCurrent({
      expectedGroupId: 'group-a',
      currentGroupId: 'group-b',
      expectedSessionId: 'session-a',
      currentSessionId: 'session-a'
    }),
    false
  );
  assert.equal(
    isBeeroomDispatchScopeCurrent({
      expectedGroupId: 'group-a',
      currentGroupId: 'group-a',
      expectedSessionId: 'session-a',
      currentSessionId: 'session-b'
    }),
    false
  );
});
