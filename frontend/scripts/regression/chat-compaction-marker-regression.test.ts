import test from 'node:test';
import assert from 'node:assert/strict';

import { mergeCompactionMarkersIntoMessages } from '../../src/stores/chatCompactionMarker';
import {
  resolveCompactionDividerStatus,
  resolveLatestCompactionSnapshot,
  isCompactionRunningFromWorkflowItems
} from '../../src/utils/chatCompactionWorkflow';

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
          toolName: 'context_compaction',
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
          toolName: 'context_compaction',
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

test('hydrated manual compaction marker suppresses cached terminal marker from same round', () => {
  const cachedMessages = [
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-04-10T01:25:41.323Z',
      stream_round: 2,
      workflowStreaming: false,
      stream_incomplete: false,
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          toolName: 'context_compaction',
          toolCallId: 'compaction:manual:1775784341323',
          detail: JSON.stringify({
            status: 'done',
            trigger_mode: 'manual',
            user_round: 2,
            projected_request_tokens: 33696,
            projected_request_tokens_after: 3623
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
      created_at: '2026-04-10T01:25:58.534Z',
      stream_round: 2,
      workflowStreaming: false,
      stream_incomplete: false,
      manual_compaction_marker: true,
      workflowItems: [
        {
          status: 'completed',
          detail: JSON.stringify({
            status: 'done',
            trigger_mode: 'manual',
            user_round: 2,
            projected_request_tokens: 33696,
            projected_request_tokens_after: 3623
          })
        }
      ]
    }
  ];

  const merged = mergeCompactionMarkersIntoMessages(remoteMessages, cachedMessages);

  assert.equal(merged.length, 1);
  assert.equal(merged[0]?.created_at, '2026-04-10T01:25:58.534Z');
});

test('completed manual compaction divider stays completed while session is busy with a new turn', () => {
  const items = [
    {
      eventType: 'compaction',
      status: 'completed',
      toolName: 'context_compaction',
      detail: JSON.stringify({
        status: 'done',
        trigger_mode: 'manual',
        user_round: 2,
        projected_request_tokens: 23934,
        projected_request_tokens_after: 5678
      })
    }
  ];

  const snapshot = resolveLatestCompactionSnapshot(items);
  const status = resolveCompactionDividerStatus({
    snapshot,
    runningFromWorkflowItems: isCompactionRunningFromWorkflowItems(items),
    manualMarker: true,
    isStreaming: false,
    sessionBusy: true
  });

  assert.equal(status, 'completed');
});

test('manual compaction divider still shows running before a terminal snapshot exists', () => {
  const status = resolveCompactionDividerStatus({
    snapshot: null,
    runningFromWorkflowItems: false,
    manualMarker: true,
    isStreaming: false,
    sessionBusy: true
  });

  assert.equal(status, 'running');
});

test('auto loop compaction with chinese tool name is still recognized as compaction', () => {
  const items = [
    {
      status: 'completed',
      toolName: '\u4e0a\u4e0b\u6587\u538b\u7f29',
      detail: JSON.stringify({
        status: 'done',
        trigger_mode: 'auto_loop',
        user_round: 5
      })
    }
  ];

  const snapshot = resolveLatestCompactionSnapshot(items);

  assert.equal(snapshot?.eventType, 'compaction');
  assert.equal(snapshot?.status, 'completed');
});

test('completed auto loop compaction divider stays completed while session is busy with a new turn', () => {
  const items = [
    {
      eventType: 'compaction',
      status: 'completed',
      toolName: 'context_compaction',
      detail: JSON.stringify({
        status: 'done',
        trigger_mode: 'auto_loop',
        user_round: 5,
        projected_request_tokens: 18888,
        projected_request_tokens_after: 6222
      })
    }
  ];

  const snapshot = resolveLatestCompactionSnapshot(items);
  const status = resolveCompactionDividerStatus({
    snapshot,
    runningFromWorkflowItems: isCompactionRunningFromWorkflowItems(items),
    manualMarker: false,
    isStreaming: false,
    sessionBusy: true
  });

  assert.equal(status, 'completed');
});

test('overflow recovery compaction divider shows running before terminal snapshot exists', () => {
  const items = [
    {
      eventType: 'compaction_progress',
      status: 'loading',
      toolName: 'context_compaction',
      detail: JSON.stringify({
        status: 'loading',
        stage: 'compacting',
        trigger_mode: 'overflow_recovery',
        user_round: 6
      })
    }
  ];

  const snapshot = resolveLatestCompactionSnapshot(items);
  const status = resolveCompactionDividerStatus({
    snapshot,
    runningFromWorkflowItems: isCompactionRunningFromWorkflowItems(items),
    manualMarker: false,
    isStreaming: false,
    sessionBusy: true
  });

  assert.equal(status, 'running');
});

test('foreground hydration restores completed manual compaction divider from watched messages', () => {
  const watchedMessages = [
    {
      role: 'user',
      content: '绘制一个爱心给我',
      created_at: '2026-04-11T12:15:00.000Z'
    },
    {
      role: 'assistant',
      content: '我来为你绘制一个爱心。',
      created_at: '2026-04-11T12:15:01.000Z'
    },
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-04-11T12:15:06.708Z',
      stream_round: 2,
      workflowStreaming: false,
      stream_incomplete: false,
      manual_compaction_marker: true,
      workflowItems: [
        {
          eventType: 'compaction',
          status: 'completed',
          toolName: 'context_compaction',
          toolCallId: 'compaction:manual:1775909704662',
          detail: JSON.stringify({
            status: 'done',
            trigger_mode: 'manual',
            user_round: 2,
            projected_request_tokens: 16249,
            projected_request_tokens_after: 5670
          })
        }
      ]
    },
    {
      role: 'user',
      content: '不错',
      created_at: '2026-04-11T12:15:15.550Z'
    },
    {
      role: 'assistant',
      content: '',
      reasoning: '',
      created_at: '2026-04-11T12:15:15.563Z',
      workflowStreaming: true,
      stream_incomplete: true,
      workflowItems: []
    }
  ];

  const foregroundMergedMessages = [
    {
      role: 'user',
      content: '绘制一个爱心给我',
      created_at: '2026-04-11T12:15:00.000Z'
    },
    {
      role: 'assistant',
      content: '我来为你绘制一个爱心。',
      created_at: '2026-04-11T12:15:01.000Z'
    },
    {
      role: 'user',
      content: '不错',
      created_at: '2026-04-11T12:15:15.550Z'
    },
    {
      role: 'assistant',
      content: '接下来还需要我调整吗？',
      reasoning: '等待用户确认是否继续调整图像。',
      created_at: '2026-04-11T12:15:15.579Z',
      stream_round: 3,
      workflowStreaming: false,
      stream_incomplete: false,
      workflowItems: []
    }
  ];

  const reconciled = mergeCompactionMarkersIntoMessages(
    foregroundMergedMessages,
    watchedMessages
  );

  assert.equal(reconciled.length, 5);
  assert.equal(reconciled.filter((message) => message.manual_compaction_marker === true).length, 1);
  assert.equal(reconciled[2]?.manual_compaction_marker, true);
  assert.equal(reconciled[2]?.workflowItems?.[0]?.eventType, 'compaction');
  assert.equal(reconciled[3]?.role, 'user');
  assert.equal(reconciled[3]?.content, '不错');
});
