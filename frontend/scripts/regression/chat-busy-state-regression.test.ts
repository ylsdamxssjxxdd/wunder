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

test('merged busy state keeps running when projection is stale idle but runtime is running', () => {
  const projection = createChatRuntimeProjection();
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_running',
    messages: [
      { role: 'user', content: 'input' },
      { role: 'assistant', content: 'partial answer' }
    ],
    loading: false,
    running: false
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_running',
      loading: false,
      messages: [
        { role: 'user', content: 'input' },
        { role: 'assistant', content: 'partial answer' }
      ],
      runtimeStatus: 'running',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    true
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_running',
      loading: false,
      messages: [
        { role: 'user', content: 'input' },
        { role: 'assistant', content: 'partial answer' }
      ],
      runtimeStatus: 'running',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'running'
  );
});

test('merged busy state suppresses stale assistant streaming after confirmed idle runtime', () => {
  const projection = createChatRuntimeProjection();
  const messages = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: 'done',
      stream_incomplete: true,
      workflowStreaming: true
    }
  ];
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_idle',
    messages,
    loading: true,
    running: true
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_idle',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_idle',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'idle'
  );
});

test('merged busy state suppresses stale busy projection after confirmed idle runtime', () => {
  const projection = createChatRuntimeProjection();
  const messages = [
    { role: 'user', content: 'input' },
    { role: 'assistant', content: 'done' }
  ];
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_stale_projection',
    messages,
    loading: true,
    running: true
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_stale_projection',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_stale_projection',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'idle'
  );
});

test('merged busy state keeps stop available while a send controller is active', () => {
  const messages = [
    { role: 'user', content: 'input' },
    { role: 'assistant', content: '', stream_incomplete: false, workflowStreaming: false }
  ];

  assert.equal(
    resolveMergedSessionBusy({
      projection: null,
      sessionId: 'sess_active_controller',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: true
    }),
    true
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection: null,
      sessionId: 'sess_active_controller',
      loading: false,
      messages,
      runtimeStatus: 'idle',
      runtimeKnown: true,
      runtimeHasControllers: true
    }),
    'running'
  );
});

test('merged busy state suppresses stale assistant streaming after completed runtime', () => {
  const projection = createChatRuntimeProjection();
  const messages = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: 'done',
      stream_incomplete: true,
      workflowStreaming: true
    }
  ];
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_completed_terminal',
    messages,
    loading: true,
    running: true
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_completed_terminal',
      loading: false,
      messages,
      runtimeStatus: 'completed',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_completed_terminal',
      loading: false,
      messages,
      runtimeStatus: 'completed',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'completed'
  );
});

test('merged busy state stays cleared across idle to not_loaded terminal chain', () => {
  const projection = createChatRuntimeProjection();
  const messages = [
    { role: 'user', content: 'input' },
    {
      role: 'assistant',
      content: 'done',
      stream_incomplete: true,
      workflowStreaming: true
    }
  ];
  applyChatRuntimeEvent(projection, {
    event_type: 'legacy_messages_reconciled',
    source: 'legacy',
    strict: false,
    session_id: 'sess_terminal_chain',
    messages,
    loading: true,
    running: true
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'session_runtime',
    source: 'legacy',
    strict: false,
    session_id: 'sess_terminal_chain',
    runtime_status: 'idle'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'session_idle',
    source: 'legacy',
    strict: false,
    session_id: 'sess_terminal_chain'
  });
  applyChatRuntimeEvent(projection, {
    event_type: 'session_runtime',
    source: 'legacy',
    strict: false,
    session_id: 'sess_terminal_chain',
    runtime_status: 'not_loaded'
  });

  assert.equal(
    resolveMergedSessionBusy({
      projection,
      sessionId: 'sess_terminal_chain',
      loading: false,
      messages,
      runtimeStatus: 'not_loaded',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    false
  );
  assert.equal(
    resolveMergedSessionRuntimeStatus({
      projection,
      sessionId: 'sess_terminal_chain',
      loading: false,
      messages,
      runtimeStatus: 'not_loaded',
      runtimeKnown: true,
      runtimeHasControllers: false
    }),
    'not_loaded'
  );
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
  assert.equal(runtime.stopRequested, false);
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
