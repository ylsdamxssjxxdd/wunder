import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createPinia, setActivePinia } from 'pinia';

import {
  shouldForcePreserveWatcherForActiveSession,
  shouldApplyForegroundDetailHydration,
  shouldKeepForegroundLiveMessages,
  shouldRestartWatchAfterInteractiveStream
} from '../../src/stores/chatWatchLifecycle';
import { createChatRuntimeProjection } from '../../src/realtime/chat/chatRuntimeReducer';
import { useCommandSessionStore } from '../../src/stores/commandSessions';

const ensureBrowserStorageMocks = () => {
  const storage = {
    getItem: () => null,
    setItem: () => undefined,
    removeItem: () => undefined
  };
  const target = globalThis as Record<string, any>;
  target.localStorage ??= storage;
  target.sessionStorage ??= storage;
  target.window ??= {
    location: { origin: 'http://localhost' },
    addEventListener: () => undefined,
    removeEventListener: () => undefined
  };
  target.document ??= {
    addEventListener: () => undefined,
    removeEventListener: () => undefined
  };
};

test('watch restarts when stream finishes on the active session', () => {
  assert.equal(
    shouldRestartWatchAfterInteractiveStream({
      activeSessionId: 'sess_demo',
      targetSessionId: 'sess_demo',
      pageUnloading: false
    }),
    true
  );
});

test('watch does not restart when active session differs', () => {
  assert.equal(
    shouldRestartWatchAfterInteractiveStream({
      activeSessionId: 'sess_a',
      targetSessionId: 'sess_b',
      pageUnloading: false
    }),
    false
  );
});

test('watch does not restart while page is unloading', () => {
  assert.equal(
    shouldRestartWatchAfterInteractiveStream({
      activeSessionId: 'sess_demo',
      targetSessionId: 'sess_demo',
      pageUnloading: true
    }),
    false
  );
});

test('foreground detail hydration remains allowed when preserveWatcher mode only has watch controller', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'watching',
      hasWatchController: true,
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('foreground detail hydration is blocked while preserveWatcher mode still has interactive lifecycle', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'sending',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false
    }),
    false
  );
});

test('foreground detail hydration is allowed once preserveWatcher mode is fully idle', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'idle',
      hasWatchController: false,
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('foreground detail hydration remains allowed when preserveWatcher mode is disabled', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: false,
      lifecycle: 'watching',
      hasWatchController: true,
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('foreground detail hydration is blocked whenever interactive stream controllers are alive', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: false,
      lifecycle: 'watching',
      hasWatchController: true,
      hasSendController: true,
      hasResumeController: false
    }),
    false
  );
});

test('foreground detail hydration can be forced for manual reconcile even during interactive lifecycle', () => {
  assert.equal(
    shouldApplyForegroundDetailHydration({
      preserveWatcher: true,
      lifecycle: 'sending',
      hasWatchController: true,
      hasSendController: true,
      hasResumeController: false,
      forceHydration: true
    }),
    true
  );
});

test('foreground sync keeps live messages only while remote stream is still running', () => {
  assert.equal(
    shouldKeepForegroundLiveMessages({
      preserveWatcher: true,
      hydrateForegroundMessages: false,
      remoteRunning: true
    }),
    true
  );
});

test('foreground sync replaces stale live messages once remote stream is idle', () => {
  assert.equal(
    shouldKeepForegroundLiveMessages({
      preserveWatcher: true,
      hydrateForegroundMessages: false,
      remoteRunning: false
    }),
    false
  );
});

test('foreground sync does not keep live messages when hydrated foreground replacement is enabled', () => {
  assert.equal(
    shouldKeepForegroundLiveMessages({
      preserveWatcher: true,
      hydrateForegroundMessages: true,
      remoteRunning: true
    }),
    false
  );
});

test('active-session detail load forces preserveWatcher while send stream is still alive', () => {
  assert.equal(
    shouldForcePreserveWatcherForActiveSession({
      isSameActiveSession: true,
      lifecycle: 'sending',
      hasSendController: false,
      hasResumeController: false
    }),
    true
  );
});

test('active-session detail load does not force preserveWatcher when only watch lifecycle is active', () => {
  assert.equal(
    shouldForcePreserveWatcherForActiveSession({
      isSameActiveSession: true,
      lifecycle: 'watching',
      hasSendController: false,
      hasResumeController: false
    }),
    false
  );
});

test('detail load never forces preserveWatcher for inactive sessions', () => {
  assert.equal(
    shouldForcePreserveWatcherForActiveSession({
      isSameActiveSession: false,
      lifecycle: 'sending',
      hasSendController: true,
      hasResumeController: false
    }),
    false
  );
});

test('watcher keeps projection events flowing while send or resume controllers exist', () => {
  const watcherSource = readFileSync(resolve(process.cwd(), 'src/stores/chatWatcher.ts'), 'utf8');
  const projectionOnlySource = readFileSync(resolve(process.cwd(), 'src/stores/chatProjectionOnlyEvents.ts'), 'utf8');
  assert.ok(projectionOnlySource.includes("'round_start'"));
  assert.ok(projectionOnlySource.includes("'channel_message'"));
  assert.ok(projectionOnlySource.includes("'tool_call_delta'"));
  assert.ok(projectionOnlySource.includes("'tool_output'"));
  assert.ok(projectionOnlySource.includes("'tool_output_delta'"));
  assert.ok(projectionOnlySource.includes("'llm_request'"));
  assert.ok(projectionOnlySource.includes("'llm_stream_retry'"));
  assert.ok(projectionOnlySource.includes("'knowledge_request'"));
  assert.ok(projectionOnlySource.includes("'thread_control'"));
  assert.ok(projectionOnlySource.includes("'command_session_start'"));
  assert.ok(projectionOnlySource.includes("'command_session_delta'"));
  assert.ok(projectionOnlySource.includes("'command_session_summary'"));
  assert.ok(projectionOnlySource.includes("'token_usage'"));
  assert.ok(projectionOnlySource.includes("'round_usage'"));
  assert.ok(projectionOnlySource.includes("'context_usage'"));
  assert.ok(projectionOnlySource.includes("'quota_usage'"));
  assert.ok(projectionOnlySource.includes("'plan_update'"));
  assert.ok(projectionOnlySource.includes("'question_panel'"));
  assert.ok(projectionOnlySource.includes("'slow_client'"));
  assert.ok(projectionOnlySource.includes("'compaction'"));
  assert.ok(projectionOnlySource.includes("'compaction_progress'"));
  assert.ok(projectionOnlySource.includes("'compaction_notice'"));
  assert.ok(!watcherSource.includes('if (runtime.sendController || runtime.resumeController) return;'));
  const canonicalIndex = watcherSource.indexOf('applyCanonicalStreamRuntimeEvent(');
  assert.ok(canonicalIndex >= 0);
  assert.ok(watcherSource.includes('selectRuntimeLastAppliedEventId'));
  assert.ok(watcherSource.includes('refreshLastAppliedEventId();'));
  assert.equal(watcherSource.includes('lastEventId = Math.max(lastEventId, normalizedEventId)'), false);
  assert.equal(watcherSource.includes('updateRuntimeLastEventId(runtime, normalizedEventId)'), false);
  assert.equal(watcherSource.includes('const shouldUseProjectionOnlyWatch ='), false);
  assert.equal(watcherSource.includes('shouldUseProjectionOnlyWatchStreamEvent('), false);
  assert.equal(watcherSource.includes('shouldUseLegacyWatchStreamFallback'), false);
  assert.equal(watcherSource.includes('resolveWatchRoundNumber'), false);
  assert.equal(watcherSource.includes('ensureRoundState'), false);
  assert.equal(watcherSource.includes('roundStates'), false);
  assert.equal(watcherSource.includes('state.processor.handleEvent('), false);
  assert.equal(watcherSource.includes('insertWatchUserMessage('), false);
});

test('send, resume, and watch no longer route realtime events through legacy processors', () => {
  const sendSource = readFileSync(resolve(process.cwd(), 'src/stores/chatSendActions.ts'), 'utf8');
  const resumeSource = readFileSync(resolve(process.cwd(), 'src/stores/chatStopResumeActions.ts'), 'utf8');
  const runtimeControlsSource = readFileSync(resolve(process.cwd(), 'src/stores/chatRuntimeControls.ts'), 'utf8');
  for (const [label, source] of [['send', sendSource], ['resume', resumeSource]]) {
    assert.ok(source.includes("from './chatProjectionOnlyEvents';"), label);
    assert.ok(source.includes('shouldUseProjectionOnlyInteractiveStreamEvent('), label);
    assert.ok(source.includes('normalizedEventType,'), label);
    assert.ok(source.includes('sideEffects: projectionOnlyInteractiveEvent'), label);
    const projectionOnlyComputeIndex = source.indexOf('const projectionOnlyInteractiveEvent = shouldUseProjectionOnlyInteractiveStreamEvent(');
    const canonicalApplyIndex = source.indexOf('applyCanonicalStreamRuntimeEvent(', projectionOnlyComputeIndex);
    assert.ok(projectionOnlyComputeIndex >= 0, label);
    assert.ok(canonicalApplyIndex > projectionOnlyComputeIndex, label);
    assert.equal(source.includes('shouldUseLegacyInteractiveStreamFallback'), false, label);
    assert.equal(source.includes('legacyInteractiveFallbackEvent'), false, label);
    assert.equal(source.includes('createWorkflowProcessor('), false, label);
    assert.equal(source.includes('processor.handleEvent('), false, label);
    assert.equal(source.includes('processor.finalize('), false, label);
    assert.equal(source.includes('assignStreamEventId('), false, label);
    assert.equal(source.includes('captureRealtimeWorkflowMutationBaseline('), false, label);
    assert.equal(source.includes('logRealtimeWorkflowMutation('), false, label);
    assert.ok(source.includes('selectRuntimeLastAppliedEventId'), label);
    assert.ok(source.includes('syncRuntimeLastAppliedEventId'), label);
    assert.equal(source.includes('updateRuntimeLastEventId(runtime, eventId)'), false, label);
    assert.ok(source.includes("normalizedEventType === 'llm_output' &&"), label);
    assert.ok(source.includes('isTerminalLlmOutputPayload(payload, approvalPayload)'), label);
  }
  const sendQueueIndex = sendSource.indexOf('if (queuedFlag) {');
  assert.ok(sendQueueIndex >= 0);
  assert.equal(sendSource.includes('assistantMessage.stream_incomplete = true'), false);
  assert.equal(sendSource.includes('assistantMessage.workflowStreaming = true'), false);
  assert.equal(sendSource.includes('if (!bootstrappingDraftSession) {\n        this.messages.push(userMessage);'), false);
  assert.equal(sendSource.includes('if (!bootstrappingDraftSession) {\n        sessionMessagesRef.push(assistantMessageRaw);'), false);
  assert.equal(sendSource.includes('scheduleSlowClientResume(this, sessionId, assistantMessage'), false);
  assert.ok(sendSource.includes('scheduleSlowClientResume(this, sessionId, null, slowClientResumeAfterEventId)'));
  assert.ok(sendSource.includes('resolveMaxProjectionUserRound(this.runtimeProjection, this.activeSessionId)'));
  assert.equal(sendSource.includes('const nextLocalStreamRound = (resolveMaxStreamRound(this.messages) || 0) + 1;'), false);
  assert.equal(runtimeControlsSource.includes('export const insertWatchUserMessage'), false);
  assert.equal(runtimeControlsSource.includes('export const ensurePendingAssistantMessage'), false);
  assert.equal(runtimeControlsSource.includes('WATCH_USER_MESSAGE_DEDUP_MS'), false);
});

test('history hydration no longer rebuilds legacy workflow state from raw stream events', () => {
  const source = readFileSync(resolve(process.cwd(), 'src/stores/chatMessageHydration.ts'), 'utf8');
  assert.equal(source.includes("import { createWorkflowProcessor } from './chatWorkflowProcessor';"), false);
  assert.equal(source.includes('createWorkflowProcessor('), false);
  assert.equal(source.includes('processor.handleEvent('), false);
  assert.equal(source.includes('processor.finalize('), false);
  assert.equal(source.includes('message.workflow_events.forEach'), false);
  const workflowHydrationSource = readFileSync(resolve(process.cwd(), 'src/stores/chatWorkflowHydration.ts'), 'utf8');
  assert.equal(workflowHydrationSource.includes('workflow_events ='), false);
  assert.equal(workflowHydrationSource.includes('buildWorkflowEventRaw'), false);
  assert.ok(source.includes('void workflowState;'));
});

test('interactive projection-only stream keeps side-effect-heavy events on canonical side effects', async () => {
  const {
    shouldUseProjectionOnlyInteractiveStreamEvent,
    shouldUseProjectionOnlyWatchStreamEvent
  } = await import('../../src/stores/chatProjectionOnlyEvents');
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('delta'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('channel_message'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('token_usage'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('llm_output'), true);
  assert.equal(
    shouldUseProjectionOnlyInteractiveStreamEvent('llm_output', { terminalLlmOutput: true }),
    true
  );
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('thread_control'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('workspace_update'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('desktop_controller_hint'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('command_session_start'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('command_session_delta'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('team_task_result'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('subagent_status'), true);
  assert.equal(shouldUseProjectionOnlyWatchStreamEvent('thread_control'), true);
  assert.equal(shouldUseProjectionOnlyWatchStreamEvent('channel_message'), true);
  assert.equal(shouldUseProjectionOnlyWatchStreamEvent('command_session_start'), true);
  assert.equal(shouldUseProjectionOnlyWatchStreamEvent('team_task_result'), true);
  assert.equal(shouldUseProjectionOnlyWatchStreamEvent('subagent_status'), true);
  assert.equal(shouldUseProjectionOnlyInteractiveStreamEvent('custom_unknown_event'), true);
  assert.equal(shouldUseProjectionOnlyWatchStreamEvent('custom_unknown_event'), true);
  const projectionOnlyModule = await import('../../src/stores/chatProjectionOnlyEvents');
  assert.equal('shouldUseLegacyInteractiveStreamFallback' in projectionOnlyModule, false);
  assert.equal('shouldUseLegacyWatchStreamFallback' in projectionOnlyModule, false);
});

test('channel messages no longer enter realtime legacy sideband writers', () => {
  const watchSource = readFileSync(resolve(process.cwd(), 'src/stores/chatWatcher.ts'), 'utf8');
  const resumeSource = readFileSync(resolve(process.cwd(), 'src/stores/chatStopResumeActions.ts'), 'utf8');
  const sendSource = readFileSync(resolve(process.cwd(), 'src/stores/chatSendActions.ts'), 'utf8');
  const projectionOnlySource = readFileSync(resolve(process.cwd(), 'src/stores/chatProjectionOnlyEvents.ts'), 'utf8');
  const oldSidebandRuntimePath = resolve(process.cwd(), 'src/stores/chatWatchChannelMessageRuntime.ts');

  assert.ok(projectionOnlySource.includes("'channel_message'"));
  assert.equal(existsSync(oldSidebandRuntimePath), false);
  for (const [label, source] of [
    ['watch', watchSource],
    ['resume', resumeSource],
    ['send', sendSource]
  ] as const) {
    assert.equal(source.includes("import { consumeChatWatchChannelMessage } from './chatWatchChannelMessageRuntime';"), false, label);
    assert.equal(source.includes('consumeChatWatchChannelMessage({'), false, label);
  }

  assert.ok(watchSource.includes('applyCanonicalStreamRuntimeEvent('));
  assert.equal(watchSource.includes('shouldUseLegacyWatchStreamFallback'), false);
  assert.equal(watchSource.includes('resolveWatchRoundNumber'), false);
  assert.equal(watchSource.includes('state.processor.handleEvent('), false);

  const resumeProjectionOnlyIndex = resumeSource.indexOf('const projectionOnlyInteractiveEvent = shouldUseProjectionOnlyInteractiveStreamEvent(');
  const resumeCanonicalIndex = resumeSource.indexOf('applyCanonicalStreamRuntimeEvent(', resumeProjectionOnlyIndex);
  assert.ok(resumeProjectionOnlyIndex >= 0);
  assert.ok(resumeCanonicalIndex > resumeProjectionOnlyIndex);
  assert.equal(resumeSource.includes('shouldUseLegacyInteractiveStreamFallback'), false);
  assert.equal(resumeSource.includes('processor.handleEvent('), false);
});

test('canonical projection-only side effects preserve command session output store', async () => {
  ensureBrowserStorageMocks();
  const { applyCanonicalStreamRuntimeEvent } = await import('../../src/stores/chatRuntimeState');
  setActivePinia(createPinia());
  const store = {
    activeSessionId: 'session-1',
    runtimeProjection: createChatRuntimeProjection(),
    runtimeProjectionVersion: 0
  } as Record<string, any>;
  const commandSessions = useCommandSessionStore();

  applyCanonicalStreamRuntimeEvent(
    store,
    'session-1',
    'command_session_start',
    {
      data: {
        command_session_id: 'cmd-1',
        command: 'run',
        status: 'running'
      }
    },
    1,
    { phase: 'send', sideEffects: true }
  );
  applyCanonicalStreamRuntimeEvent(
    store,
    'session-1',
    'tool_output_delta',
    {
      data: {
        command_session_id: 'cmd-1',
        stream: 'stdout',
        delta: 'chunk'
      }
    },
    2,
    { phase: 'send', sideEffects: true }
  );

  const entry = commandSessions.entries['cmd-1'];
  assert.ok(entry);
  assert.equal(entry.sessionId, 'session-1');
  assert.equal(entry.command, 'run');
  assert.equal(entry.stdoutTail, 'chunk');
  assert.equal(entry.stdoutBytes, 5);
});

test('canonical projection-only side effects preserve collaboration refresh semantics', async () => {
  ensureBrowserStorageMocks();
  const { applyCanonicalStreamRuntimeEvent, sessionSubagentsCache } = await import('../../src/stores/chatRuntimeState');
  const store = {
    activeSessionId: 'session-1',
    runtimeProjection: createChatRuntimeProjection(),
    runtimeProjectionVersion: 0
  } as Record<string, any>;
  const agentRefreshEvents: Array<Record<string, unknown>> = [];
  const previousDispatch = globalThis.window.dispatchEvent;
  globalThis.window.dispatchEvent = ((event: Event) => {
    if (event instanceof CustomEvent && event.type === 'wunder:agent-runtime-refresh') {
      agentRefreshEvents.push(event.detail ?? {});
    }
    return true;
  }) as typeof window.dispatchEvent;
  try {
    sessionSubagentsCache.set('session-1', { cachedAt: 1, items: [{}] });
    applyCanonicalStreamRuntimeEvent(
      store,
      'session-1',
      'subagent_status',
      {
        data: {
          run_id: 'child-1',
          status: 'running'
        }
      },
      3,
      { phase: 'send', sideEffects: true }
    );
    assert.equal(sessionSubagentsCache.has('session-1'), false);

    applyCanonicalStreamRuntimeEvent(
      store,
      'session-1',
      'team_task_result',
      {
        data: {
          task_id: 'task-1',
          agent_id: 'agent-worker'
        }
      },
      4,
      { phase: 'send', sideEffects: true }
    );
    assert.deepEqual(agentRefreshEvents.at(-1), { agentIds: ['agent-worker'] });
  } finally {
    globalThis.window.dispatchEvent = previousDispatch;
  }
});

test('canonical projection-only side effects preserve workspace update semantics without render events', async () => {
  ensureBrowserStorageMocks();
  const { applyCanonicalStreamRuntimeEvent } = await import('../../src/stores/chatRuntimeState');
  const store = {
    activeSessionId: 'session-1',
    runtimeProjection: createChatRuntimeProjection(),
    runtimeProjectionVersion: 0
  } as Record<string, any>;
  const workspaceEvents: Array<Record<string, unknown>> = [];
  const previousDispatch = globalThis.window.dispatchEvent;
  globalThis.window.dispatchEvent = ((event: Event) => {
    if (event instanceof CustomEvent && event.type === 'wunder:workspace-refresh') {
      workspaceEvents.push(event.detail ?? {});
    }
    return true;
  }) as typeof window.dispatchEvent;
  try {
    applyCanonicalStreamRuntimeEvent(
      store,
      'session-1',
      'workspace_update',
      {
        data: {
          session_id: 'session-1',
          workspace_id: 'workspace-1',
          agent_id: 'agent-1',
          changed_paths: ['output.txt'],
          tree_version: 7
        }
      },
      5,
      { phase: 'send', sideEffects: true }
    );
    assert.deepEqual(workspaceEvents.at(-1), {
      sessionId: 'session-1',
      workspaceId: 'workspace-1',
      agentId: 'agent-1',
      containerId: null,
      treeVersion: 7,
      reason: 'workspace_update',
      path: 'output.txt',
      paths: ['output.txt']
    });
  } finally {
    globalThis.window.dispatchEvent = previousDispatch;
  }
});
