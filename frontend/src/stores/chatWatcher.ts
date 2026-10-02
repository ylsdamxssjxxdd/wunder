import { applyChatThreadEffects, syncChatThreadShell } from './chatThreadEffects';
import { advanceThreadLogCursor, threadLogCursor } from './chatThreadCursor';
import { selectVisibleMessageProjections } from '@/realtime/chat/chatRuntimeSelectors';
import {
  applyChatThreadServerEvent,
  ensureChatThreadRuntime,
  getChatThreadState,
  registerChatThreadSnapshotLoader
} from '@/realtime/chat/chatThreadRuntime';
import { isChatSnapshotCurrent } from './chatSnapshotFreshness';
import { defineStore } from 'pinia';

import {
  archiveSession as archiveSessionApi,
  cancelMessageStream,
  compactSession as compactSessionApi,
  controlSessionSubagents as controlSessionSubagentsApi,
  createSession,
  deleteSession as deleteSessionApi,
  getSession,
  getSessionGoal,
  getSessionEvents,
  getSessionHistoryPage,
  getSessionSubagents,
  getThreadLogSnapshot,
  getThreadLogTurn,
  listSessions,
  openChatSocket,
  renameSession as renameSessionApi,
  restoreSession as restoreSessionApi,
  setSessionGoal as setSessionGoalApi,
  submitMessageFeedback as submitMessageFeedbackApi,
  updateSessionTools as updateSessionToolsApi
} from '@/api/chat';
import { t } from '@/i18n';
import { formatStructuredErrorText } from '@/utils/streamError';
import { resolveCompactionProgressTitle } from '@/utils/chatCompactionUi';
import {
  buildChatRequestTextInputOverflowError,
  resolveChatRequestTextInputOverflow
} from '@/utils/chatRequestInputLimit';
import {
  hasActiveSubagentsAfterLatestUser,
  hasRunningAssistantMessage,
  hasStreamingAssistantMessage,
  isSessionBusyFromSignals,
  isThreadRuntimeBusy,
  isThreadRuntimeWaiting,
  normalizeThreadRuntimeStatus
} from '@/utils/chatSessionRuntime';
import {
  isSubagentItemActive,
  normalizeSubagentRuntimeFlag,
  isSubagentStatusFailed,
  isSubagentStatusSuccessful,
  normalizeSubagentRuntimeStatus
} from '@/utils/subagentRuntime';
import { normalizeChatDurationSeconds, normalizeChatTimestampMs } from '@/utils/chatTiming';
import {
  mergeSessionsByIdPreservingRuntimeFields
} from '@/stores/chatSessionMerge';
import {
  estimateChatTextTokens,
  estimateRequestContextTokens,
  resolveRequestContextPreviewTokens
} from '@/utils/chatContextEstimate';
import { resolveWorkflowDurationMs } from '@/utils/toolWorkflowTiming';
import { summarizeTurnDecodeSpeed } from '@/utils/turnDecodeSpeed';
import {
  normalizeMessageFeedback,
  normalizeMessageFeedbackVote
} from '@/utils/messageFeedback';
import { createWsMultiplexer } from '@/utils/ws';
import { isDemoMode, loadDemoChatState, saveDemoChatState } from '@/utils/demo';
import { emitAgentRuntimeRefresh, emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { chatPerf } from '@/utils/chatPerf';
import { chatDebugLog, isChatDebugEnabled, isChatDebugVerboseEnabled } from '@/utils/chatDebug';
import { buildMessageIdentityDebugList } from '@/utils/chatMessageDebug';
import { getDesktopToolCallModeForRequest, isDesktopModeEnabled } from '@/config/desktop';
import { resolveAccessToken } from '@/api/requestAuth';
import {
  createChatRuntimeProjection,
  applyChatRuntimeEvent
} from '@/realtime/chat/chatRuntimeReducer';
import {
  selectSessionBusy,
  selectSessionBusyReason,
  selectRuntimeLastAppliedEventId,
  selectSessionRuntimeStatus
} from '@/realtime/chat/chatRuntimeSelectors';
import type { ChatRuntimeProjection } from '@/realtime/chat/chatRuntimeTypes';
import {
  clearTrailingPendingAssistantMessages,
  findPendingAssistantMessage,
  isPendingAssistantMessage,
  stopPendingAssistantMessage
} from './chatPendingMessage';
import {
  captureChatSnapshotScheduleContext,
  resolveChatSnapshotScheduleSource
} from './chatSnapshotScheduler';
import { resolveInteractiveControllerRecoveryReason } from './chatInteractiveRuntimeRecovery';
import {
  normalizeStreamLifecyclePhase,
  shouldForcePreserveWatcherForActiveSession,
  shouldApplyForegroundDetailHydration,
  shouldKeepForegroundInteractiveRuntime,
  shouldKeepForegroundLiveMessagesDuringRunningGap,
  shouldKeepForegroundLiveMessages,
  shouldRestartWatchAfterInteractiveStream
} from './chatWatchLifecycle';
import { isCompactionSummaryEvent } from '@/utils/chatCompactionWorkflow';
import {
  dedupeTerminalCompactionMarkersInPlace,
  isCompactionMarkerAssistantMessage,
  isSupersededRunningManualCompactionMarker,
  mergeCompactionMarkersIntoMessages,
  shouldPreserveTerminalCompactionMarkerState
} from './chatCompactionMarker';
import {
  replaceMessageArrayKeepingReference,
  resolveRealtimeMessageArrayReference
} from './chatMessageArraySync';
import { useCommandSessionStore } from './commandSessions';
import { hasRetainedMessageConversationContext as hasRetainedConversationContext } from '@/views/messenger/messageConversationRetention';

import { buildWorkflowItem, hydrateSessionCommandSessions, safeJsonParse } from './chatDemoPanels';
import { applyGoalStreamEvent } from './chatPersist';
import { SLOW_CLIENT_RESUME_DELAY_MS, WATCH_RECONCILE_COOLDOWN_MS, WATCH_RECONCILE_DELAY_MS, abortWatchStream, clearRuntimeInteractiveControllers, clearRuntimeResumeStreamState, clearRuntimeSendStreamState, clearSessionWatcher, clearSlowClientResume, clearWatchdog, recoverRuntimeInteractiveControllers, resolveWatchdogProfile, setSessionLoading } from './chatRuntimeControls';
import { applyCanonicalSessionEventsSnapshot, applyCanonicalStreamRuntimeEvent, applySessionRuntimeEvent, applySessionRuntimeSnapshot, buildLatestAssistantRuntimeDebugSnapshot, buildRuntimeDebugSnapshot, cacheSessionMessages, syncChatRuntimeProjectionFromSnapshot, clearRuntimeProjectionInvalidation, clearSessionEventsSnapshot, countAssistantStreamingMessages, ensureRuntime, getRuntime, getSessionMessages, hasKnownSessionInStore, isSessionUnavailableStatus, loadSessionEventsSnapshot, notifySessionSnapshot, purgeUnavailableSession, refreshRuntimeStreamLifecycle, resolveChatHttpStatus, resolveSessionKey, resolveSessionMessageArray, sessionDetailPrefetchInFlight, sessionDetailSnapshotCache, sessionDetailWarmState, sessionEventsSnapshotCache, sessionEventsSnapshotInFlight, sessionHistoryState, sessionHydratedMessageVersion, sessionListCache, sessionListCacheInFlight, sessionMessages, sessionProtectedRealtimeMessages, sessionRuntime, sessionRuntimeShadowState, sessionSubagentsCache, sessionSubagentsInFlight } from './chatRuntimeState';
import { settleTerminalAssistantArtifacts as settleTerminalAssistantArtifactsBase } from './chatTerminalArtifacts';
import { chatWatcherSharedState } from './chatSharedState';
import { clearAllChatSnapshots, clearScheduledChatSnapshot } from './chatSnapshot';
import { buildMessage } from './chatStats';
import { getRuntimeLastEventId, normalizeFlag, normalizeStreamEventId, updateRuntimeLastEventId, updateRuntimeRemoteLastEventId } from './chatStreamIds';
import { buildDetail, handleApprovalEvent, isTerminalLlmOutputPayload, isTerminalStreamEventType, resolveNormalizedStreamEventType, sessionWorkflowState } from './chatWorkflowHydration';

export const startSessionWatcher = (store, sessionId) => {
  clearSessionWatcher();
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  const desktopMode = isDesktopModeEnabled();
  if (!hasKnownSessionInStore(store, key)) {
    purgeUnavailableSession(store, key);
    return;
  }
  chatWatcherSharedState.sessionWatchSessionId = key;
  const runtime = ensureRuntime(key);
  if (!runtime) return;
  // Change-stream v2 state lives per session; the watch cursor resumes from
  // its lastSeq so reconnects never replay what the reducer already applied.
  ensureChatThreadRuntime(key);
  recoverRuntimeInteractiveControllers(store, key, runtime);
  refreshRuntimeStreamLifecycle(runtime);
  runtime.watchController = new AbortController();
  runtime.watchActiveRoundCount = 0;
  refreshRuntimeStreamLifecycle(runtime);
  const controller = runtime.watchController;
  runtime.watchLastEventAt = Date.now();
  runtime.watchReconcileAt = 0;
  const requestId = buildWsRequestId();
  runtime.watchRequestId = requestId;
  let sessionMessagesRef = resolveSessionMessageArray(store, key, store.messages);
  cacheSessionMessages(key, sessionMessagesRef);
  let threadResumeTimer: ReturnType<typeof setTimeout> | null = null;
  controller.signal.addEventListener('abort', () => {
    if (threadResumeTimer) clearTimeout(threadResumeTimer);
  }, { once: true });

  const resumeWatchFromLastSeq = () => {
    if (controller.signal.aborted || threadResumeTimer) return;
    threadResumeTimer = setTimeout(() => {
      threadResumeTimer = null;
      if (controller.signal.aborted || runtime.watchController !== controller) return;
      startSessionWatcher(store, key);
    }, 0);
  };

  const startWatchdog = () => {
    if (desktopMode || runtime.watchdogTimer) return;
    const tick = () => {
      runtime.watchdogTimer = setTimeout(() => {
        runtime.watchdogTimer = null;
        if (controller.signal.aborted) return;
        const profile = resolveWatchdogProfile(store, key);
        const lastEventAt = Number(runtime.watchLastEventAt) || 0;
        // Once v2 has accepted any durable turn, its turn state is the only
        // liveness authority. `sessionMessagesRef` intentionally retains
        // optimistic legacy placeholders for compatibility actions; treating
        // one of those placeholders as live after a durable terminal change
        // repeatedly restarted watch, which in turn overlapped feeders.
        const threadState = getChatThreadState(key);
        const durableRunning = threadState && threadState.turns.size > 0
          ? Array.from(threadState.turns.values()).some((turn) => {
              const status = String(turn.status ?? '').trim().toLowerCase();
              return !['completed', 'failed', 'cancelled', 'interrupted', 'rejected', 'stopped'].includes(status);
            })
          : null;
        const running = durableRunning ?? (
          isThreadRuntimeBusy(runtime?.threadStatus) ||
          hasRunningAssistantMessage(sessionMessagesRef)
        );
        if (running && lastEventAt && Date.now() - lastEventAt >=
            Math.max(Number(profile.idleMs) || 0, WATCHDOG_V2_LIVENESS_IDLE_MS) &&
            !runtime.sendController && !runtime.resumeController) {
          if (chatPerf.enabled()) chatPerf.count('chat_watch_liveness_reconnect', 1, { sessionId: key });
          controller.abort();
          startSessionWatcher(store, key);
          return;
        }
        tick();
      }, Math.max(0, Number(resolveWatchdogProfile(store, key).intervalMs) || 0));
    };
    tick();
  };

  const onEvent = (eventType, dataText) => {
    runtime.watchLastEventAt = Date.now();
    const payload = safeJsonParse(dataText);
    const data = payload?.data ?? payload;
    const normalizedEventType = resolveNormalizedStreamEventType(eventType, payload);
    if (normalizedEventType !== 'heartbeat' && normalizedEventType !== 'ping') {
      clearSessionEventsSnapshot(key, { keepInFlight: true });
    }
    if (applyGoalStreamEvent(store, key, normalizedEventType, data ?? payload)) return;
    if (applyChatThreadServerEvent(store, key, normalizedEventType || eventType, payload, {
      onChangesApplied: (changes) => applyChatThreadEffects(store, key, changes),
      onSnapshotRequired: () => controller.abort(),
      onSnapshotApplied: () => { syncChatThreadShell(store, key); resumeWatchFromLastSeq(); },
      onOverflow: resumeWatchFromLastSeq,
      onGapOverflow: resumeWatchFromLastSeq
    })) return;

    // Only non-timeline controls remain outside the durable reducer.
    if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
      applySessionRuntimeEvent(store, key, data ?? payload, normalizedEventType);
      if (['completed', 'failed', 'cancelled', 'interrupted', 'rejected', 'stopped'].includes(String(data?.status ?? data?.thread_status ?? '').toLowerCase())) {
        setSessionLoading(store, key, false);
      }
      return;
    }
    if (normalizedEventType === 'heartbeat' || normalizedEventType === 'ping') return;
    handleApprovalEvent(store, normalizedEventType || eventType, data, requestId, key);
  };

  startWatchdog();
  const watchPromise = chatWsClient.request({
    requestId,
    sessionId: key,
    message: () => ({
      type: 'watch', request_id: requestId, session_id: key,
      payload: { after_change_seq: getChatThreadState(key)?.lastSeq ?? 0 }
    }),
    onEvent,
    signal: controller.signal,
    closeOnFinal: false,
    // Session-level stop owns cancellation. Aborting this local watcher must
    // not emit an additional request-scoped cancel frame.
    cancelOnAbort: false
  });
  watchPromise
    .catch((error) => {
      store.clearPendingApprovals({ requestId, sessionId: key });
      if (error?.name === 'AbortError' || error?.phase === 'aborted') return;
      if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
        purgeUnavailableSession(store, key);
        return;
      }
      if (error?.phase !== 'connect' && error?.phase !== 'stream' && error?.name !== 'TypeError') {
        setSessionLoading(store, key, false);
      }
    })
    .finally(() => {
      const current = getRuntime(key);
      if (current && current.watchController === controller) {
        current.watchController = null;
        current.watchActiveRoundCount = 0;
        current.watchRequestId = null;
        clearWatchdog(current);
        refreshRuntimeStreamLifecycle(current);
        if (chatWatcherSharedState.sessionWatchSessionId === key) chatWatcherSharedState.sessionWatchSessionId = '';
      }
      if (!controller.signal.aborted && store.activeSessionId === key && !desktopMode) {
        setTimeout(() => startSessionWatcher(store, key), 80);
      }
    });
};
export const chatWsClient = createWsMultiplexer(() => openChatSocket(), {
  idleTimeoutMs: 30000,
  connectTimeoutMs: 10000,
  pingIntervalMs: 20000,
  readyTimeoutMs: 150,
  onConnecting: () => undefined,
  onReady: () => undefined
});

// v2 liveness probe threshold (plan §5 M2-A): a running session silent past
// this floor gets its watch reconnected; idle sessions are left alone.
const WATCHDOG_V2_LIVENESS_IDLE_MS = 6000;

// Atomic thread-log snapshot loader for the v2 pipeline (plan §5 M2-C). The
// runtime module stays API-free; this module owns the real implementation and
// tests can replace it via registerChatThreadSnapshotLoader.
registerChatThreadSnapshotLoader(async (sessionKey) => {
  const response = await getThreadLogSnapshot(sessionKey);
  const body = response?.data?.data ?? response?.data ?? {};
  const cursor = Number(body?.cursor);
  if (!Number.isSafeInteger(cursor) || cursor < 0) {
    throw new Error('thread snapshot cursor missing');
  }
  return {
    cursor,
    turns: Array.isArray(body?.turns) ? body.turns : [],
    items: Array.isArray(body?.items) ? body.items : [],
    blocks: Array.isArray(body?.blocks) ? body.blocks : []
  };
});

let wsRequestSeq = 0;

export const buildWsRequestId = () => {
  wsRequestSeq = (wsRequestSeq + 1) % 1000000;
  return `req_${Date.now().toString(36)}_${wsRequestSeq}`;
};

export const abortResumeStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  clearSlowClientResume(runtime);
  clearRuntimeResumeStreamState(runtime, { abort: true, abortReason: 'teardown' });
  refreshRuntimeStreamLifecycle(runtime);
};

export const abortSendStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  clearSlowClientResume(runtime);
  clearRuntimeSendStreamState(runtime, { abort: true, abortReason: 'teardown' });
  refreshRuntimeStreamLifecycle(runtime);
};

export const abortCompactRequest = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  if (runtime.compactController) {
    runtime.compactController.abort();
    runtime.compactController = null;
  }
};

export const isAbortRequestError = (error: unknown): boolean => {
  const name = String((error as { name?: unknown })?.name || '').trim().toLowerCase();
  const code = String((error as { code?: unknown })?.code || '').trim().toLowerCase();
  const message = String((error as { message?: unknown })?.message || '').trim().toLowerCase();
  if (name === 'aborterror' || name === 'cancelerror' || name === 'cancelederror') {
    return true;
  }
  if (code === 'err_canceled' || code === 'abort_err') {
    return true;
  }
  if (!message) return false;
  return message === 'canceled' || message === 'cancelled' || message.includes('abort');
};

export const resolveCompactionWorkflowRefFromMessage = (message): string => {
  const items = Array.isArray(message?.workflowItems) ? message.workflowItems : [];
  for (let cursor = items.length - 1; cursor >= 0; cursor -= 1) {
    const item = items[cursor];
    const ref = String(item?.toolCallId || item?.tool_call_id || '').trim();
    if (!ref || !ref.startsWith('compaction:')) continue;
    const eventType = String(item?.eventType || item?.event || '').trim().toLowerCase();
    if (
      eventType === 'compaction' ||
      eventType === 'compaction_progress' ||
      eventType === 'compaction_notice'
    ) {
      return ref;
    }
  }
  return `compaction:manual:${Date.now()}`;
};

export const buildPendingManualCompactionMarkerMessage = (
  createdAt: number = Date.now(),
  workflowRef = `compaction:manual:${createdAt}`
) => ({
  ...buildMessage('assistant', '', createdAt),
  workflowItems: [
    buildWorkflowItem(
      t('chat.workflow.compactionRunning'),
      buildDetail({
        stage: 'compacting',
        status: 'loading',
        summary: t('chat.workflow.compactionRunning'),
        trigger_mode: 'manual'
      }),
      'loading',
      {
        isTool: true,
        eventType: 'compaction_progress',
        toolName: 'compaction',
        toolCallId: workflowRef
      }
    )
  ],
  workflowStreaming: true,
  reasoningStreaming: false,
  stream_incomplete: true,
  manual_compaction_marker: true
});

export const findRunningManualCompactionMarkerMessage = (messages) => {
  if (!Array.isArray(messages) || messages.length === 0) return null;
  for (let cursor = messages.length - 1; cursor >= 0; cursor -= 1) {
    const message = messages[cursor];
    if (!isCompactionMarkerAssistantMessage(message)) continue;
    if (message?.manual_compaction_marker !== true && message?.manualCompactionMarker !== true) continue;
    if (!normalizeFlag(message?.workflowStreaming) && !normalizeFlag(message?.stream_incomplete)) continue;
    return message;
  }
  return null;
};

export const finalizeManualCompactionAsCancelled = (message): void => {
  if (!message || message.role !== 'assistant') return;
  const cancelledDetail = buildDetail({
    stage: 'compacting',
    status: 'cancelled',
    trigger_mode: 'manual',
    error_code: 'MANUAL_COMPACTION_CANCELLED',
    error_message: t('chat.workflow.abortedDetail')
  });
  if (!Array.isArray(message.workflowItems)) {
    message.workflowItems = [];
  }
  if (message.workflowItems.length > 0) {
    message.workflowItems[0].status = 'completed';
    message.workflowItems[0].detail = cancelledDetail;
  }
  const hasCompactionTerminal = message.workflowItems.some(
    (item) => String(item?.eventType || '').trim().toLowerCase() === 'compaction'
  );
  if (!hasCompactionTerminal) {
    message.workflowItems.push(
      buildWorkflowItem(
        t('chat.toolWorkflow.compaction.title'),
        cancelledDetail,
        'completed',
        {
          isTool: true,
          eventType: 'compaction',
          toolName: 'compaction',
          toolCallId: resolveCompactionWorkflowRefFromMessage(message)
        }
      )
    );
  }
  message.workflowStreaming = false;
  message.reasoningStreaming = false;
  message.stream_incomplete = false;
  message.resume_available = false;
  message.content = '';
};

export const finalizeManualCompactionAsRequestFailed = (message, error): void => {
  if (!message || message.role !== 'assistant') return;
  const detailText = String(
    error?.response?.data?.detail || error?.message || t('common.requestFailed')
  ).trim();
  const failedDetail = buildDetail({
    stage: 'context_overflow_recovery',
    status: 'failed',
    trigger_mode: 'manual',
    error_code: String(error?.response?.data?.code || error?.code || 'MANUAL_COMPACTION_FAILED'),
    error_message: detailText
  });
  if (!Array.isArray(message.workflowItems)) {
    message.workflowItems = [];
  }
  if (message.workflowItems.length > 0) {
    message.workflowItems[0].status = 'failed';
    message.workflowItems[0].detail = failedDetail;
    (message.workflowItems[0] as Record<string, unknown>).eventType = 'compaction';
  }
  const hasCompactionTerminal = message.workflowItems.some(
    (item) => String(item?.eventType || '').trim().toLowerCase() === 'compaction'
  );
  if (!hasCompactionTerminal) {
    message.workflowItems.push(
      buildWorkflowItem(
        t('chat.toolWorkflow.compaction.title'),
        failedDetail,
        'failed',
        {
          isTool: true,
          eventType: 'compaction',
          toolName: 'compaction',
          toolCallId: resolveCompactionWorkflowRefFromMessage(message)
        }
      )
    );
  }
  message.workflowStreaming = false;
  message.reasoningStreaming = false;
  message.stream_incomplete = false;
  message.resume_available = false;
  message.content = '';
};

export const resetChatRuntimeState = () => {
  Array.from(sessionRuntime.keys()).forEach((sessionId) => {
    abortResumeStream(sessionId);
    abortSendStream(sessionId);
    abortCompactRequest(sessionId);
    abortWatchStream(sessionId);
  });
  useCommandSessionStore().reset();
  clearSessionWatcher();
  sessionRuntime.clear();
  sessionMessages.clear();
  sessionProtectedRealtimeMessages.clear();
  sessionListCache.clear();
  sessionListCacheInFlight.clear();
  sessionEventsSnapshotCache.clear();
  sessionEventsSnapshotInFlight.clear();
  sessionDetailSnapshotCache.clear();
  sessionHydratedMessageVersion.clear();
  sessionDetailPrefetchInFlight.clear();
  sessionSubagentsInFlight.clear();
  sessionSubagentsCache.clear();
  sessionDetailWarmState.clear();
  sessionHistoryState.clear();
  sessionRuntimeShadowState.clear();
  clearRuntimeProjectionInvalidation();
  sessionWorkflowState.clear();
  clearScheduledChatSnapshot();
  clearAllChatSnapshots();
};
