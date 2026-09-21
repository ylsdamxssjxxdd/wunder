import test from 'node:test';
import assert from 'node:assert/strict';

import {
  createChatRuntimeProjection,
  applyChatRuntimeEvent
} from '../../src/realtime/chat/chatRuntimeReducer';
import { hasAssistantWaitingForCurrentOutput } from '../../src/utils/assistantMessageRuntime';
import {
  resolveMergedSessionBusy,
  resolveMergedSessionRuntimeStatus
} from '../../src/stores/chatBusyState';
import { settleTerminalAssistantArtifacts } from '../../src/stores/chatTerminalArtifacts';
import { settleStoppedRuntimeLocalState } from '../../src/stores/chatRuntimeStopSettlement';

const installBrowserStorageStub = () => {
  if (typeof globalThis.localStorage !== 'undefined') return;
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: () => null,
      setItem: () => undefined,
      removeItem: () => undefined,
      clear: () => undefined
    }
  });
};

test('canonical running survives stale local idle and controller cleanup', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, { event_type: 'session_runtime', session_id: 'session-1',
    runtime_status: 'running', strict: false });
  const options = { projection, sessionId: 'session-1', runtimeStatus: 'idle',
    runtimeKnown: true, loading: false, runtimeHasControllers: false, messages: [] };
  assert.deepEqual([resolveMergedSessionBusy(options), resolveMergedSessionRuntimeStatus(options)],
    [true, 'running']);
  applyChatRuntimeEvent(projection, { event_type: 'session_idle', session_id: 'session-1', strict: false });
  assert.deepEqual([resolveMergedSessionBusy({ ...options, loading: true, runtimeHasControllers: true }),
    resolveMergedSessionRuntimeStatus(options)], [false, 'idle']);
});

test('queued canonical state allows another message without becoming running', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, { event_type: 'session_runtime', session_id: 'session-1',
    runtime_status: 'queued', strict: false });
  const options = { projection, sessionId: 'session-1', runtimeStatus: 'running', loading: true };
  assert.deepEqual([resolveMergedSessionBusy(options), resolveMergedSessionRuntimeStatus(options)],
    [false, 'queued']);
});

test('confirmed terminal projection wins over stale streaming artifacts', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, { event_type: 'session_idle', session_id: 'session-1', strict: false });
  assert.equal(resolveMergedSessionBusy({ projection, sessionId: 'session-1',
    runtimeStatus: 'running', messages: [{ role: 'assistant', workflowStreaming: true }] }), false);
});

test('terminal settle clears stale assistant waiting artifacts, workflow items, and subagents', () => {
  const waitingUpdatedAtMs = Date.now() - 1000;
  const messages = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: '',
      workflowStreaming: true,
      stream_incomplete: true,
      reasoningStreaming: false,
      waiting_updated_at_ms: waitingUpdatedAtMs,
      waiting_first_output_at_ms: null,
      stats: {},
      workflowItems: [
        { eventType: 'tool_call', status: 'loading' },
        { eventType: 'tool_result', status: 'completed' }
      ],
      subagents: [
        {
          run_id: 'run_demo',
          status: 'running',
          terminal: false,
          failed: false,
          canTerminate: true,
          updated_at_ms: 100
        }
      ]
    }
  ];

  assert.equal(settleTerminalAssistantArtifacts(messages, { failed: false }), true);
  assert.equal(messages[1].workflowItems[0].status, 'completed');
  assert.equal(messages[1].workflowItems[1].status, 'completed');
  assert.equal(messages[1].subagents[0].status, 'completed');
  assert.equal(messages[1].subagents[0].terminal, true);
  assert.equal(messages[1].subagents[0].canTerminate, false);
  assert.equal(messages[1].workflowStreaming, false);
  assert.equal(messages[1].stream_incomplete, false);
  assert.equal(hasAssistantWaitingForCurrentOutput(messages[1]), false);
});

test('terminal settle clears status-only streaming placeholders after recovery', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: '',
      status: 'streaming',
      workflowStreaming: false,
      stream_incomplete: false,
      reasoningStreaming: false,
      slow_client: false,
      resume_available: false
    }
  ];

  assert.equal(settleTerminalAssistantArtifacts(messages, { failed: false }), true);
  assert.equal(messages[1].status, 'final');
  assert.equal(messages[1].final, true);
  assert.equal(messages[1].failed, false);
  assert.equal(messages[1].workflowStreaming, false);
  assert.equal(messages[1].stream_incomplete, false);
  assert.equal(messages[1].reasoningStreaming, false);
  assert.equal(hasAssistantWaitingForCurrentOutput(messages[1]), false);
  assert.equal(
    resolveMergedSessionBusy({
      projection: null,
      sessionId: 'sess_terminal_status_only',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
});

test('terminal settle clears queued assistant placeholders after stream completion', () => {
  const messages: Record<string, any>[] = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: '# Final response',
      status: 'queued',
      workflowStreaming: true,
      stream_incomplete: true,
      reasoningStreaming: false,
      workflowItems: [{ eventType: 'queued', status: 'queued' }]
    }
  ];

  assert.equal(settleTerminalAssistantArtifacts(messages, { failed: false }), true);
  assert.equal(messages[1].status, 'final');
  assert.equal(messages[1].final, true);
  assert.equal(messages[1].workflowStreaming, false);
  assert.equal(messages[1].stream_incomplete, false);
  assert.equal(messages[1].workflowItems[0].status, 'completed');
  assert.equal(hasAssistantWaitingForCurrentOutput(messages[1]), false);
});

test('user stop settlement clears local runtime locks that would keep composer busy', () => {
  const sessionId = 'sess_user_stop_local_settle';
  const waitingUpdatedAtMs = Date.now() - 1000;
  const sendController = new AbortController();
  const resumeController = new AbortController();
  const watchController = new AbortController();
  const compactController = new AbortController();
  const messages = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: '',
      workflowStreaming: true,
      stream_incomplete: true,
      waiting_updated_at_ms: waitingUpdatedAtMs,
      waiting_first_output_at_ms: null,
      stats: {}
    }
  ];
  const runtime = {
    sendController,
    resumeController,
    watchController,
    compactController,
    watchActiveRoundCount: 1,
    activeTurnId: 'turn_running',
    pendingApprovalIds: ['approval_running'],
    pendingApprovalCount: 1,
    waitingForUserInput: true,
    stopRequested: true,
    threadStatus: 'running',
    loaded: true,
    streamLifecycle: 'watching',
    sendAbortReason: '',
    resumeAbortReason: ''
  };

  assert.equal(settleStoppedRuntimeLocalState(runtime, { abortReason: 'user_stop' }), true);
  assert.equal(sendController.signal.aborted, true);
  assert.equal(resumeController.signal.aborted, true);
  assert.equal(watchController.signal.aborted, true);
  assert.equal(compactController.signal.aborted, true);
  assert.equal(runtime.sendController, null);
  assert.equal(runtime.resumeController, null);
  assert.equal(runtime.watchController, null);
  assert.equal(runtime.compactController, null);
  assert.equal(runtime.watchActiveRoundCount, 0);
  assert.equal(runtime.activeTurnId, '');
  assert.equal(runtime.pendingApprovalCount, 0);
  assert.equal(runtime.waitingForUserInput, false);
  assert.equal(runtime.stopRequested, true);
  assert.equal(runtime.threadStatus, 'idle');
  assert.equal(runtime.streamLifecycle, 'idle');
  assert.equal(
    resolveMergedSessionBusy({
      projection: null,
      sessionId,
      loading: false,
      messages,
      runtimeStatus: runtime.threadStatus,
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
});

test('stale send finalizer cannot clear a newer send controller', async () => {
  installBrowserStorageStub();
  const { clearRuntimeSendStreamState } = await import('../../src/stores/chatRuntimeControls');
  const nextSendController = new AbortController();
  const runtime = {
    sendController: nextSendController,
    sendRequestId: 'req_next_send',
    sendStartedAt: 100,
    sendLastEventAt: 120,
    sendAbortReason: ''
  };

  assert.equal(clearRuntimeSendStreamState(runtime, { requestId: 'req_old_send' }), false);
  assert.equal(runtime.sendController, nextSendController);
  assert.equal(nextSendController.signal.aborted, false);
  assert.equal(runtime.sendRequestId, 'req_next_send');
  assert.equal(runtime.sendStartedAt, 100);
  assert.equal(runtime.sendLastEventAt, 120);

  assert.equal(clearRuntimeSendStreamState(runtime, { requestId: 'req_next_send' }), true);
  assert.equal(runtime.sendController, null);
  assert.equal(runtime.sendRequestId, null);
  assert.equal(runtime.sendStartedAt, 0);
  assert.equal(runtime.sendLastEventAt, 0);
});
