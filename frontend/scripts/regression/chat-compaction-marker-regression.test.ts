import test from 'node:test';
import assert from 'node:assert/strict';

import { mergeCompactionMarkersIntoMessages } from '../../src/stores/chatCompactionMarker';

test('remote terminal manual compaction marker replaces cached running marker', () => {
  const cachedMessages = [
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-04-09T07:18:30.000Z',
      stream_round: 3,
      workflowStreaming: true,
      stream_incomplete: true,
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction_progress',
          status: 'loading',
          toolName: '上下文压缩',
          toolCallId: 'compaction:manual:123',
          detail: JSON.stringify({
            status: 'loading',
            stage: 'compacting',
            trigger_mode: 'manual',
            user_round: 3
          })
        }
      ]
    }
  ];

  const remoteMessages = [
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-04-09T07:18:31.000Z',
      stream_round: 3,
      workflowStreaming: false,
      stream_incomplete: false,
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          toolName: '上下文压缩',
          toolCallId: 'compaction:manual:123',
          detail: JSON.stringify({
            status: 'done',
            trigger_mode: 'manual',
            user_round: 3
          })
        }
      ]
    }
  ];

  const merged = mergeCompactionMarkersIntoMessages(remoteMessages, cachedMessages);

  assert.equal(merged.length, 1);
  assert.equal(merged[0]?.workflowStreaming, false);
  assert.equal(merged[0]?.stream_incomplete, false);
  assert.equal(merged[0]?.workflowItems?.[0]?.eventType, 'compaction');
});
