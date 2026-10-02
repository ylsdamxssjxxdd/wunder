import test from 'node:test';
import assert from 'node:assert/strict';
import { createWorkspaceHydrationBatch } from '../../src/utils/workspaceHydrationBatch';

test('concurrent markdown renders hydrate every requested message once', () => {
  const batch = createWorkspaceHydrationBatch();
  batch.add(['first']);
  batch.add(['second', 'first']);
  batch.add(['third']);
  assert.deepEqual(batch.take(), { messageKeys: ['first', 'second', 'third'] });
  batch.add(['next-frame']);
  assert.deepEqual(batch.take(), { messageKeys: ['next-frame'] });
});

test('whole viewport refresh includes rows arriving before and after it', () => {
  const batch = createWorkspaceHydrationBatch();
  batch.add(['first']);
  batch.add();
  batch.add(['second']);
  assert.deepEqual(batch.take(), {});
});

test('large batches collapse to a bounded viewport scan and page reset clears keys', () => {
  const batch = createWorkspaceHydrationBatch();
  batch.add(Array.from({ length: 129 }, (_, i) => `row-${i}`));
  assert.deepEqual(batch.take(), {});
  batch.add();
  batch.clear();
  batch.add(['new-page']);
  assert.deepEqual(batch.take(), { messageKeys: ['new-page'] });
});
