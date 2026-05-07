import test from 'node:test';
import assert from 'node:assert/strict';

import { shouldRenderWorkflowShell } from '../../src/components/chat/toolWorkflowVisibility';

test('workflow shell stays visible while pending placeholder exists before entries are parsed', () => {
  assert.equal(
    shouldRenderWorkflowShell({
      visible: true,
      entryCount: 0,
      hasPendingPlaceholder: true
    }),
    true
  );
});

test('workflow shell stays hidden when there are no entries and no pending placeholder', () => {
  assert.equal(
    shouldRenderWorkflowShell({
      visible: true,
      entryCount: 0,
      hasPendingPlaceholder: false
    }),
    false
  );
});
