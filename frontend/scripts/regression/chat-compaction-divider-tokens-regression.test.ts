import test from 'node:test';
import assert from 'node:assert/strict';

import { resolveCompactionDividerTransitionTokens } from '../../src/components/chat/compactionDividerTokens';

test('compaction divider ignores non-decreasing projected request transitions', () => {
  const transition = resolveCompactionDividerTransitionTokens({
    projected_request_tokens: 2402,
    projected_request_tokens_after: 2585,
    context_tokens: 2402,
    context_tokens_after: 2585,
    final_context_tokens: 2585
  });

  assert.equal(transition, null);
});

test('compaction divider prefers decreasing message-context transition', () => {
  const transition = resolveCompactionDividerTransitionTokens({
    context_tokens: 4547,
    context_tokens_after: 2641,
    projected_request_tokens: 4547,
    projected_request_tokens_after: 2641
  });

  assert.deepEqual(transition, {
    before: 4547,
    after: 2641
  });
});
