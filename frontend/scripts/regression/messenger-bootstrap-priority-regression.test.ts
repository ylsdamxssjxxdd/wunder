import test from 'node:test';
import assert from 'node:assert/strict';

import {
  shouldUseNonBlockingDesktopMessageBootstrap,
  splitMessengerBootstrapTasks
} from '../../src/views/messenger/bootstrap';

test('desktop message startup does not block the painted shell on session hydration', () => {
  assert.equal(shouldUseNonBlockingDesktopMessageBootstrap(true, 'messages'), true);
  assert.equal(shouldUseNonBlockingDesktopMessageBootstrap(true, 'tools'), false);
  assert.equal(shouldUseNonBlockingDesktopMessageBootstrap(false, 'messages'), false);
});

test('section bootstrap keeps scoped tasks critical outside desktop fast start', () => {
  const messageTask = { sections: ['messages'] as const, run: async () => undefined };
  const backgroundTask = { run: async () => undefined };
  const { critical, background } = splitMessengerBootstrapTasks('messages', [
    messageTask,
    backgroundTask
  ]);

  assert.deepEqual(critical, [messageTask]);
  assert.deepEqual(background, [backgroundTask]);
});
