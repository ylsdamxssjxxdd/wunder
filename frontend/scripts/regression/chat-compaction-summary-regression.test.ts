import test from 'node:test';
import assert from 'node:assert/strict';

import { isCompactionSummaryEvent } from '../../src/utils/chatCompactionWorkflow';

test('compaction summary model request is classified separately from normal llm events', () => {
  assert.equal(
    isCompactionSummaryEvent('llm_request', { purpose: 'compaction_summary' }),
    true
  );
  assert.equal(
    isCompactionSummaryEvent('llm_output', { purpose: 'compaction_summary' }),
    true
  );
  assert.equal(
    isCompactionSummaryEvent('llm_request', { purpose: 'normal_reply' }),
    false
  );
  assert.equal(
    isCompactionSummaryEvent('round_usage', { purpose: 'compaction_summary' }),
    false
  );
});
