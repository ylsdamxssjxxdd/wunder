import test from 'node:test';
import assert from 'node:assert/strict';
import { ref } from 'vue';

import { useMessengerInteractionBlocker } from '../../src/views/messenger/interactionBlocker';

const waitFor = (ms: number) =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, ms);
  });

test('interaction blocker blocks concurrent actions and blurs focused input in messenger root', async () => {
  let blurred = false;
  const focusedElement = {
    blur: () => {
      blurred = true;
    }
  } as unknown as HTMLElement;
  const rootElement = {
    contains: (target: unknown) => target === focusedElement
  } as unknown as HTMLElement;

  const originalDocument = (globalThis as { document?: Document }).document;
  (globalThis as { document?: Document }).document = {
    activeElement: focusedElement
  } as unknown as Document;

  try {
    const rootRef = ref<HTMLElement | null>(rootElement);
    const blocker = useMessengerInteractionBlocker({
      rootRef,
      resolveLabel: (reason) => reason,
      minVisibleMs: 0
    });

    const running = blocker.runWithBlock('refresh', async () => {
      await waitFor(25);
      return 'ok';
    });
    assert.equal(blocker.isBlocked.value, true);
    assert.equal(blocker.label.value, 'refresh');

    const rejectedByBlock = await blocker.runWithBlock('new_session', async () => 'secondary');
    assert.equal(rejectedByBlock, null);

    const result = await running;
    assert.equal(result, 'ok');
    assert.equal(blocker.isBlocked.value, false);
    assert.equal(blurred, true);
  } finally {
    (globalThis as { document?: Document }).document = originalDocument;
  }
});
