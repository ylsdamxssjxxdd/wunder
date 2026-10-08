import test from 'node:test';
import assert from 'node:assert/strict';

import { buildApplyPatchEmptyPreviewText } from '../../src/components/chat/toolWorkflowPatchPreview';

test('apply_patch delete preview uses an action-oriented placeholder instead of a missing diff warning', () => {
  assert.equal(
    buildApplyPatchEmptyPreviewText('delete'),
    '- whole-file delete; inline diff appears after the tool result arrives'
  );
});

test('apply_patch empty preview text remains explicit for other patch actions', () => {
  assert.equal(
    buildApplyPatchEmptyPreviewText('move'),
    '> move/rename only; no inline line diff in patch body'
  );
  assert.equal(
    buildApplyPatchEmptyPreviewText('add'),
    '+ no inline added lines in patch body'
  );
  assert.equal(
    buildApplyPatchEmptyPreviewText('update'),
    '~ no inline hunk lines in patch body'
  );
});
