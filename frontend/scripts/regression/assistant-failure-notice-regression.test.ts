import test from 'node:test';
import assert from 'node:assert/strict';

import { buildAssistantDisplayContent } from '../../src/utils/assistantFailureNotice';

const messages: Record<string, string> = {
  'chat.message.failedInlineTitle': 'This reply did not complete',
  'chat.message.failedInlineReason': 'Reason: {detail}',
  'chat.message.failedInlinePartial': 'Partial output below',
  'chat.workflow.aborted': 'Aborted',
  'chat.workflow.abortedDetail': 'Request aborted',
  'chat.workflow.requestFailed': 'Request failed',
  'chat.workflow.error': 'Error',
  'chat.workflow.requestFailedDetail': 'Request failed, please try again later'
};

const translate = (key: string, named?: Record<string, unknown>): string => {
  const template = messages[key] || key;
  return template.replace(/\{(\w+)\}/g, (_token, name: string) => String(named?.[name] ?? ''));
};

test('omits partial output when message content only repeats the full untruncated error detail', () => {
  const detail =
    'Model request failed: LLM stream request failed: 429 Too Many Requests '
    + '{"error":{"message":"Daily quota exceeded for model qwen/qwen3-235b-a22b-instruct-2507. '
    + 'Please try again tomorrow or choose another model.","request_id":"b6927b44-df96-4e6e-9b97-928561f69ab9"}}';
  const rendered = buildAssistantDisplayContent(
    {
      role: 'assistant',
      content: detail,
      workflowItems: [{ status: 'failed', detail }]
    },
    translate
  );
  assert.match(rendered, /This reply did not complete/);
  assert.match(rendered, /Reason: /);
  assert.doesNotMatch(rendered, /Partial output below/);
  assert.doesNotMatch(rendered, /request_id":"b6927b44-df96-4e6e-9b97-928561f69ab9"/);
});

test('trims a trailing full error line from partial output even when the displayed reason is truncated', () => {
  const detail =
    'Model request failed: LLM stream request failed: 429 Too Many Requests '
    + '{"error":{"message":"Daily quota exceeded for model qwen/qwen3-235b-a22b-instruct-2507. '
    + 'Please try again tomorrow or choose another model.","request_id":"b6927b44-df96-4e6e-9b97-928561f69ab9"}}';
  const rendered = buildAssistantDisplayContent(
    {
      role: 'assistant',
      content: `Partial summary before the failure.\n${detail}`,
      workflowItems: [{ status: 'failed', detail }]
    },
    translate
  );
  assert.match(rendered, /Partial output below/);
  assert.match(rendered, /Partial summary before the failure\./);
  assert.doesNotMatch(rendered, /request_id":"b6927b44-df96-4e6e-9b97-928561f69ab9"/);
});

test('hides failure notice while a compaction recovery is still running', () => {
  const rendered = buildAssistantDisplayContent(
    {
      role: 'assistant',
      content: '',
      workflowItems: [
        {
          status: 'failed',
          detail: 'upstream timeout'
        },
        {
          eventType: 'compaction_progress',
          status: 'loading'
        }
      ]
    },
    translate
  );

  assert.equal(rendered, '');
});
