import test from 'node:test';
import assert from 'node:assert/strict';

import { dedupeTerminalCompactionMarkersInPlace } from '../../src/stores/chatCompactionMarker';

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
