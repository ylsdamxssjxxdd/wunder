import test from 'node:test';
import assert from 'node:assert/strict';

import { dedupeTerminalCompactionMarkersInPlace } from '../../src/stores/chatCompactionMarker';
import { resolveCompactionDividerTransitionTokens } from '../../src/components/chat/compactionDividerTokens';
import { buildCompactionDisplay } from '../../src/utils/chatCompactionUi';

const createTranslator = () => {
  const table: Record<string, string> = {
    'chat.toolWorkflow.compaction.detail.waitingObservedUsage': 'Waiting for model usage',
    'chat.toolWorkflow.compaction.usage.afterPending': 'Waiting for model usage'
  };
  return (key: string, params?: Record<string, unknown>) => {
    const template = table[key] || key;
    if (!params) return template;
    return Object.entries(params).reduce(
      (output, [name, value]) => output.replace(`{${name}}`, String(value)),
      template
    );
  };
};

test('dedupeTerminalCompactionMarkersInPlace removes duplicate completed manual compaction markers', () => {
  const messages = [
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-05-07T10:00:00.000Z',
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          toolName: 'context_compaction',
          toolCallId: 'compaction:manual:demo',
          detail: JSON.stringify({
            status: 'done',
            trigger_mode: 'manual',
            projected_request_tokens: 4547,
            projected_request_tokens_after: 2641
          })
        }
      ]
    },
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-05-07T10:00:01.000Z',
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          toolName: 'context_compaction',
          toolCallId: 'compaction:manual:demo',
          detail: JSON.stringify({
            status: 'done',
            trigger_mode: 'manual',
            projected_request_tokens: 4547,
            projected_request_tokens_after: 2641
          })
        }
      ]
    }
  ];

  const deduped = dedupeTerminalCompactionMarkersInPlace(messages);

  assert.equal(deduped.length, 1);
  assert.equal(deduped[0]?.workflowItems?.[0]?.toolCallId, 'compaction:manual:demo');
});

test('compaction UI treats zero after-tokens as unobserved after compaction', () => {
  const detail = {
    reason: 'overflow',
    status: 'done',
    projected_request_tokens: 13409,
    projected_request_tokens_after: 0,
    context_tokens: 13409,
    context_tokens_after: 0,
    context_usage_source: 'provider_observed',
    context_usage_source_after: 'unobserved_after_compaction',
    max_context: 22000
  };

  assert.equal(resolveCompactionDividerTransitionTokens(detail), null);

  const display = buildCompactionDisplay(detail, 'completed', createTranslator());
  assert.match(display.summaryTitle, /13,409 tokens → Waiting for model usage/);
  assert.doesNotMatch(display.resultBody, /0 tokens/);
  assert.match(display.resultBody, /Waiting for model usage/);
  assert.equal(display.view.usageBar?.afterLabel, 'Waiting for model usage');
});

test('dedupeTerminalCompactionMarkersInPlace removes duplicate markers that share one compaction id', () => {
  const messages = [
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-05-07T14:07:15.000Z',
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          detail: JSON.stringify({
            compaction_id: 'cmp_manual_demo',
            trigger_mode: 'manual',
            projected_request_tokens: 3166,
            projected_request_tokens_after: 2711
          })
        }
      ]
    },
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-05-07T14:08:03.000Z',
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          toolCallId: 'compaction:4:1',
          detail: JSON.stringify({
            compaction_id: 'cmp_manual_demo',
            trigger_mode: 'manual',
            projected_request_tokens: 3166,
            projected_request_tokens_after: 2711
          })
        }
      ]
    }
  ];

  const deduped = dedupeTerminalCompactionMarkersInPlace(messages);

  assert.equal(deduped.length, 1);
});
