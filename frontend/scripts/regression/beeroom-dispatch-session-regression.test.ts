import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveNextBeeroomMotherDispatchSessionId,
  resolvePreferredBeeroomDispatchSessionId,
  shouldCacheBeeroomDispatchPreviewSnapshot,
  shouldFinishBeeroomTerminalHydration,
  shouldRestoreCachedBeeroomDispatchPreview,
  shouldPreserveBeeroomDispatchPreviewOnSyncError
} from '../../src/components/beeroom/beeroomDispatchSessionPolicy';
import { overlayBeeroomLiveDispatchLabel } from '../../src/components/beeroom/beeroomDispatchPreviewOverlay';
import {
  resolveBeeroomSwarmWorkerReplyFromHistoryMessages,
  resolveBeeroomSwarmWorkerTerminalState
} from '../../src/components/beeroom/beeroomSwarmWorkerShadowState';
import { resolveBeeroomDispatchTaskLabel } from '../../src/components/beeroom/canvas/swarmCanvasModel';

test('mother dispatch keeps following current primary session instead of stale previous session', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-old',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: 'sess-main',
    hasExplicitPrimarySession: true
  });
  assert.equal(sessionId, 'sess-main');
});

test('mother dispatch keeps the bound session when explicit main thread is temporarily unavailable', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-old',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: '',
    hasExplicitPrimarySession: false
  });
  assert.equal(sessionId, 'sess-old');
});

test('mother dispatch stays on the bound session while explicit main thread is still unknown', () => {
  const sessionId = resolvePreferredBeeroomDispatchSessionId({
    targetRole: 'mother',
    targetAgentId: 'mother-1',
    previousSessionId: 'sess-current',
    previousTargetAgentId: 'mother-1',
    activeSessionId: 'sess-active',
    primarySessionId: 'sess-main-candidate',
    hasExplicitPrimarySession: false
  });
  assert.equal(sessionId, 'sess-current');
});

test('worker dispatch only reuses previous session when it belongs to the same worker target', () => {
  assert.equal(
    resolvePreferredBeeroomDispatchSessionId({
      targetRole: 'worker',
      targetAgentId: 'worker-2',
      previousSessionId: 'sess-worker-2',
      previousTargetAgentId: 'worker-2',
      activeSessionId: 'sess-active-worker-2',
      primarySessionId: 'sess-main-worker-2'
    }),
    'sess-worker-2'
  );
  assert.equal(
    resolvePreferredBeeroomDispatchSessionId({
      targetRole: 'worker',
      targetAgentId: 'worker-2',
      previousSessionId: 'sess-other-worker',
      previousTargetAgentId: 'worker-1',
      activeSessionId: 'sess-active-worker-2',
      primarySessionId: 'sess-main-worker-2'
    }),
    'sess-active-worker-2'
  );
});

test('dispatch preview is preserved only for transient sync errors on the same session', () => {
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 0,
      currentPreviewSessionId: 'sess_1',
      requestedSessionId: 'sess_1'
    }),
    true
  );
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 500,
      currentPreviewSessionId: 'sess_1',
      requestedSessionId: 'sess_1'
    }),
    true
  );
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 404,
      currentPreviewSessionId: 'sess_1',
      requestedSessionId: 'sess_1'
    }),
    false
  );
  assert.equal(
    shouldPreserveBeeroomDispatchPreviewOnSyncError({
      status: 500,
      currentPreviewSessionId: 'sess_old',
      requestedSessionId: 'sess_new'
    }),
    false
  );
});

test('only active dispatch previews are cached and restored for beeroom canvas recovery', () => {
  assert.equal(
    shouldCacheBeeroomDispatchPreviewSnapshot({
      previewStatus: 'running',
      subagentStatuses: []
    }),
    true
  );
  assert.equal(
    shouldCacheBeeroomDispatchPreviewSnapshot({
      previewStatus: 'completed',
      subagentStatuses: ['completed']
    }),
    false
  );
  assert.equal(
    shouldRestoreCachedBeeroomDispatchPreview({
      localRuntimeStatus: 'idle',
      previewStatus: 'running',
      subagentStatuses: ['running']
    }),
    false
  );
  assert.equal(
    shouldRestoreCachedBeeroomDispatchPreview({
      localRuntimeStatus: 'running',
      previewStatus: 'running',
      subagentStatuses: ['running']
    }),
    true
  );
});

test('mother reconcile keeps the current mother session until an explicit main thread exists', () => {
  assert.equal(
    resolveNextBeeroomMotherDispatchSessionId({
      motherAgentId: 'mother-1',
      currentSessionId: 'sess-current',
      currentSessionAgentId: 'mother-1',
      explicitPrimarySessionId: '',
      fallbackPrimarySessionId: 'sess-main-candidate'
    }),
    'sess-current'
  );
  assert.equal(
    resolveNextBeeroomMotherDispatchSessionId({
      motherAgentId: 'mother-1',
      currentSessionId: 'sess-current',
      currentSessionAgentId: 'mother-1',
      explicitPrimarySessionId: 'sess-main',
      fallbackPrimarySessionId: 'sess-main-candidate'
    }),
    'sess-main'
  );
});

test('terminal beeroom hydration waits for the expected final reply instead of any assistant signature drift', () => {
  assert.equal(
    shouldFinishBeeroomTerminalHydration({
      expectedReplyText: '最终回复',
      expectedReplyMatched: false,
      baselineAssistantSignature: '1|10|k1|中间回复',
      assistantSignature: '2|12|k2|中间回复'
    }),
    false
  );
  assert.equal(
    shouldFinishBeeroomTerminalHydration({
      expectedReplyText: '最终回复',
      expectedReplyMatched: true,
      baselineAssistantSignature: '1|10|k1|中间回复',
      assistantSignature: '3|15|k3|最终回复'
    }),
    true
  );
  assert.equal(
    shouldFinishBeeroomTerminalHydration({
      expectedReplyText: '',
      expectedReplyMatched: false,
      baselineAssistantSignature: '1|10|k1|旧回复',
      assistantSignature: '2|12|k2|新回复'
    }),
    true
  );
});

test('swarm worker reply extraction reads structured assistant history payloads', () => {
  const reply = resolveBeeroomSwarmWorkerReplyFromHistoryMessages([
    {
      role: 'assistant',
      content: [
        {
          type: 'text',
          text: '{"answer":"工蜂已完成法规梳理"}'
        }
      ]
    }
  ]);
  assert.equal(reply, '工蜂已完成法规梳理');
});

test('swarm worker terminal resolution keeps workflow-active sessions running', () => {
  const runningState = resolveBeeroomSwarmWorkerTerminalState({
    currentStatus: 'awaiting_idle',
    running: false,
    events: [],
    workflowItems: [
      {
        status: 'loading',
        eventType: 'tool_call'
      }
    ]
  });
  assert.deepEqual(runningState, {
    status: 'running',
    terminal: false,
    failed: false
  });
});

test('swarm worker terminal resolution prefers the latest successful terminal event over earlier errors', () => {
  const completedState = resolveBeeroomSwarmWorkerTerminalState({
    currentStatus: 'running',
    running: false,
    events: [
      {
        event: 'error',
        data: {
          message: 'intermediate tool error'
        }
      },
      {
        event: 'final',
        data: {
          answer: 'worker done'
        }
      },
      {
        event: 'turn_terminal',
        data: {
          status: 'completed'
        }
      }
    ],
    workflowItems: [
      {
        status: 'failed',
        eventType: 'tool_call'
      },
      {
        status: 'completed',
        eventType: 'final'
      }
    ]
  });
  assert.deepEqual(completedState, {
    status: 'completed',
    terminal: true,
    failed: false
  });
});

test('live dispatch preview overlays the current outgoing label while the round is active', () => {
  const preview = overlayBeeroomLiveDispatchLabel(
    {
      sessionId: 'sess_active',
      targetAgentId: 'worker-1',
      targetName: 'Worker 1',
      status: 'running',
      summary: 'old summary',
      dispatchLabel: 'previous task',
      updatedTime: 100,
      subagents: []
    },
    {
      currentSessionId: 'sess_active',
      runtimeStatus: 'running',
      composerSending: true,
      dispatchLabelPreview: 'current task'
    }
  );
  assert.equal(preview?.dispatchLabel, 'current task');
});

test('live dispatch preview does not leak a draft label into another session', () => {
  const preview = overlayBeeroomLiveDispatchLabel(
    {
      sessionId: 'sess_other',
      targetAgentId: 'worker-1',
      targetName: 'Worker 1',
      status: 'running',
      summary: 'old summary',
      dispatchLabel: 'previous task',
      updatedTime: 100,
      subagents: []
    },
    {
      currentSessionId: 'sess_active',
      runtimeStatus: 'running',
      composerSending: true,
      dispatchLabelPreview: 'current task'
    }
  );
  assert.equal(preview?.dispatchLabel, 'previous task');
});

test('active dispatch label ignores stale terminal task summaries when current mission text is generic', () => {
  assert.equal(
    resolveBeeroomDispatchTaskLabel(
      {
        summary: 'direct send',
        strategy: 'direct_send'
      } as never,
      {
        task_id: 'task_12345678',
        status: 'completed',
        result_summary: 'previous finished task'
      } as never,
      { allowTerminalTaskFallback: false }
    ),
    '#task_123'
  );
});
