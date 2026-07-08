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
  listSessions,
  openChatSocket,
  renameSession as renameSessionApi,
  restoreSession as restoreSessionApi,
  setSessionGoal as setSessionGoalApi,
  submitMessageFeedback as submitMessageFeedbackApi,
  updateSessionTools as updateSessionToolsApi
} from '@/api/chat';
import { t } from '@/i18n';
import { setDefaultSession } from '@/api/agents';
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
  normalizeMessageFeedbackVote,
  resolveMessageHistoryId
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
import { SLOW_CLIENT_RESUME_DELAY_MS, WATCH_RECONCILE_COOLDOWN_MS, WATCH_RECONCILE_DELAY_MS, abortWatchStream, clearRuntimeInteractiveControllers, clearRuntimeResumeStreamState, clearRuntimeSendStreamState, clearSessionWatcher, clearSlowClientResume, clearWatchdog, recoverRuntimeInteractiveControllers, resolveLastAssistantStreamEventId, resolveLastStreamEventId, resolveMaxStreamEventId, resolveWatchdogProfile, setSessionLoading } from './chatRuntimeControls';
import { applyCanonicalSessionEventsSnapshot, applyCanonicalStreamRuntimeEvent, applySessionRuntimeEvent, applySessionRuntimeSnapshot, buildLatestAssistantRuntimeDebugSnapshot, buildRuntimeDebugSnapshot, cacheSessionMessages, clearRuntimeProjectionInvalidation, clearSessionEventsSnapshot, countAssistantStreamingMessages, ensureRuntime, getRuntime, getSessionMessages, hasKnownSessionInStore, isSessionUnavailableStatus, loadSessionEventsSnapshot, notifySessionSnapshot, purgeUnavailableSession, refreshRuntimeStreamLifecycle, resolveChatHttpStatus, resolveSessionKey, resolveSessionMessageArray, sessionDetailPrefetchInFlight, sessionDetailSnapshotCache, sessionDetailWarmState, sessionEventsSnapshotCache, sessionEventsSnapshotInFlight, sessionHistoryState, sessionHydratedMessageVersion, sessionListCache, sessionListCacheInFlight, sessionMessages, sessionProtectedRealtimeMessages, sessionRuntime, sessionRuntimeShadowState, sessionSubagentsCache, sessionSubagentsInFlight } from './chatRuntimeState';
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
  recoverRuntimeInteractiveControllers(store, key, runtime);
  refreshRuntimeStreamLifecycle(runtime);
  const perfEnabled = chatPerf.enabled();
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
  const tailEventId =
    resolveLastStreamEventId(sessionMessagesRef) ||
    resolveLastAssistantStreamEventId(sessionMessagesRef) ||
    resolveMaxStreamEventId(sessionMessagesRef) ||
    0;
  const hasProjectionSession = Boolean(store?.runtimeProjection?.sessions?.[key]);
  const projectionLastEventId = selectRuntimeLastAppliedEventId(store?.runtimeProjection, key);
  const runtimeLastEventId = getRuntimeLastEventId(runtime);
  const runtimeRemoteLastEventId = normalizeStreamEventId(runtime?.remoteLastEventId) || 0;
  let lastEventId = hasProjectionSession
    ? Math.max(projectionLastEventId, runtimeLastEventId, runtimeRemoteLastEventId, tailEventId)
    : Math.max(runtimeLastEventId, runtimeRemoteLastEventId, tailEventId);

  const refreshLastAppliedEventId = () => {
    const appliedEventId = selectRuntimeLastAppliedEventId(store?.runtimeProjection, key);
    if (appliedEventId > lastEventId) {
      lastEventId = appliedEventId;
      updateRuntimeLastEventId(runtime, appliedEventId);
    }
    return lastEventId;
  };

  const markWatchdogEvent = () => {
    runtime.watchLastEventAt = Date.now();
  };

  // Reconcile from server when watch events appear out-of-sync with stream state.
  const scheduleWatchReconcile = (delayMs = WATCH_RECONCILE_DELAY_MS) => {
    if (desktopMode) return;
    if (controller.signal.aborted) return;
    if (store.activeSessionId !== key) return;
    if (!hasKnownSessionInStore(store, key)) return;
    const localLastEventId = refreshLastAppliedEventId();
    recoverRuntimeInteractiveControllers(store, key, runtime, {
      localLastEventId
    });
    const bypassCooldown = Math.max(0, Number(delayMs) || 0) === 0;
    const now = Date.now();
    const nextAllowedAt = Number(runtime.watchReconcileAt) || 0;
    if (!bypassCooldown && nextAllowedAt > now) {
      return;
    }
    runtime.watchReconcileAt = now + WATCH_RECONCILE_COOLDOWN_MS;
    if (runtime.watchReconcileTimer) {
      return;
    }
    runtime.watchReconcileTimer = setTimeout(() => {
      runtime.watchReconcileTimer = null;
      if (controller.signal.aborted) return;
      if (runtime.watchController !== controller) return;
      if (store.activeSessionId !== key) return;
      if (!hasKnownSessionInStore(store, key)) return;
      void store.loadSessionDetail(key, { preserveWatcher: true }).catch(() => {});
    }, Math.max(0, Number(delayMs) || 0));
  };

  const startWatchdog = () => {
    if (desktopMode) return;
    if (runtime.watchdogTimer) return;
    const scheduleNext = (delayMs) => {
      if (controller.signal.aborted) return;
      runtime.watchdogTimer = setTimeout(() => {
        runtime.watchdogTimer = null;
        void runWatchdogTick();
      }, Math.max(0, Number(delayMs) || 0));
    };
    const runWatchdogTick = async () => {
      if (controller.signal.aborted) return;
      const profile = resolveWatchdogProfile(store, key);
      const localLastEventId = refreshLastAppliedEventId();
      recoverRuntimeInteractiveControllers(store, key, runtime, {
        localLastEventId
      });
      if (runtime.sendController || runtime.resumeController || runtime.watchdogBusy) {
        scheduleNext(profile.intervalMs);
        return;
      }
      const lastEventAt = Number(runtime.watchLastEventAt) || 0;
      if (!lastEventAt || Date.now() - lastEventAt < profile.idleMs) {
        scheduleNext(profile.intervalMs);
        return;
      }
      runtime.watchdogBusy = true;
      try {
        if (!hasKnownSessionInStore(store, key)) {
          purgeUnavailableSession(store, key);
          return;
        }
        let response = null;
        try {
          response = await loadSessionEventsSnapshot(key, {
            allowCached: false
          });
        } catch (error) {
          if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
            purgeUnavailableSession(store, key);
            return;
          }
        }
        const payload = response;
        hydrateSessionCommandSessions(key, payload?.command_sessions ?? payload?.commandSessions);
        applySessionRuntimeSnapshot(runtime, payload?.runtime);
        applyCanonicalSessionEventsSnapshot(store, key, payload, {
          phase: 'watchdog'
        });
        const localLastEventId = refreshLastAppliedEventId();
        const running = payload?.running;
        const remoteLastEventId = Number(payload?.last_event_id ?? payload?.lastEventId);
        updateRuntimeRemoteLastEventId(runtime, remoteLastEventId);
        recoverRuntimeInteractiveControllers(store, key, runtime, {
          remoteRunning: running,
          remoteLastEventId,
          localLastEventId
        });
        const shouldReconcileRemoteDrift =
          Number.isFinite(remoteLastEventId) && remoteLastEventId > localLastEventId;
        if (shouldReconcileRemoteDrift) {
          scheduleWatchReconcile(running === false ? 0 : WATCH_RECONCILE_DELAY_MS);
        }
        if (running === false) {
          clearRuntimeInteractiveControllers(runtime, { abort: false });
          const settledTerminalArtifacts = settleTerminalAssistantArtifactsBase(sessionMessagesRef);
          setSessionLoading(store, key, false);
          if (settledTerminalArtifacts) {
            notifySessionSnapshot(store, key, sessionMessagesRef, true);
          }
          if (perfEnabled) {
            chatPerf.count('chat_watchdog_idle_complete', 1, { sessionId: key });
          }
        } else if (perfEnabled) {
          chatPerf.count('chat_watchdog_idle', 1, { sessionId: key });
        }
      } finally {
        runtime.watchdogBusy = false;
        if (!controller.signal.aborted && runtime.watchController === controller) {
          const nextProfile = resolveWatchdogProfile(store, key);
          scheduleNext(nextProfile.intervalMs);
        }
      }
    };
    const initialProfile = resolveWatchdogProfile(store, key);
    scheduleNext(initialProfile.intervalMs);
  };

  const onEvent = (eventType, dataText, eventId) => {
    const currentSessionMessagesRef = resolveSessionMessageArray(store, key, sessionMessagesRef);
    if (currentSessionMessagesRef !== sessionMessagesRef) {
      sessionMessagesRef = replaceMessageArrayKeepingReference(
        currentSessionMessagesRef,
        sessionMessagesRef
      );
      cacheSessionMessages(key, sessionMessagesRef);
    }
    recoverRuntimeInteractiveControllers(store, key, runtime, {
      localLastEventId: lastEventId
    });
    refreshRuntimeStreamLifecycle(runtime);
    markWatchdogEvent();
    const payload = safeJsonParse(dataText);
    const data = payload?.data ?? payload;
    const normalizedEventType = resolveNormalizedStreamEventType(eventType, payload);
    if (normalizedEventType !== 'heartbeat' && normalizedEventType !== 'ping') {
      clearSessionEventsSnapshot(key, { keepInFlight: true });
    }
    if (applyGoalStreamEvent(store, key, normalizedEventType, data ?? payload)) {
      return;
    }
    applyCanonicalStreamRuntimeEvent(
      store,
      key,
      normalizedEventType || eventType,
      payload,
      eventId,
      {
        requestId,
        phase: 'watch',
        onSyncRequired: (reason) =>
          scheduleWatchReconcile(reason === 'event_seq_gap' ? 0 : WATCH_RECONCILE_DELAY_MS)
      }
    );
    const normalizedEventId = normalizeStreamEventId(eventId);
    if (normalizedEventId !== null) {
      updateRuntimeRemoteLastEventId(runtime, normalizedEventId);
    }
    refreshLastAppliedEventId();
    if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
      chatDebugLog('chat.store.terminal-debug', 'watch-runtime-event', {
        sessionId: key,
        eventType: normalizedEventType,
        eventId: normalizedEventId,
        payloadStatus: String(data?.thread_status ?? data?.status ?? payload?.thread_status ?? payload?.status ?? '')
          .trim()
          .toLowerCase(),
        loadingBySession: Boolean(store?.loadingBySession?.[key]),
        runtimeBefore: buildRuntimeDebugSnapshot(runtime),
        streamingAssistantCount: countAssistantStreamingMessages(sessionMessagesRef),
        latestAssistant: buildLatestAssistantRuntimeDebugSnapshot(sessionMessagesRef),
        ...(isChatDebugVerboseEnabled()
          ? { messages: buildMessageIdentityDebugList(sessionMessagesRef) }
          : {})
      });
      applySessionRuntimeEvent(store, key, data ?? payload, normalizedEventType);
      return;
    }
    if (normalizedEventType === 'heartbeat' || normalizedEventType === 'ping') {
      return;
    }
    if (perfEnabled) {
      chatPerf.count('chat_watch_event', 1, { eventType: normalizedEventType || eventType, sessionId: key });
    }
    handleApprovalEvent(store, normalizedEventType || eventType, data, requestId, key);
    const projectionTerminal =
      isTerminalStreamEventType(normalizedEventType) ||
      (normalizedEventType === 'llm_output' && isTerminalLlmOutputPayload(payload, data));
    if (projectionTerminal) {
      setSessionLoading(store, key, false);
      if (perfEnabled) {
        chatPerf.count('chat_watch_terminal', 1, {
          eventType: normalizedEventType || eventType,
          sessionId: key
        });
      }
    }
    return;
  };

  const baseEventId = lastEventId || 0;
  startWatchdog();
  const watchPromise = chatWsClient.request({
      requestId,
      sessionId: key,
      message: {
        type: 'watch',
        request_id: requestId,
        session_id: key,
        payload: { after_event_id: baseEventId }
      },
      onEvent,
      signal: controller.signal,
      closeOnFinal: false
    });
  watchPromise
    .catch((error) => {
      store.clearPendingApprovals({ requestId, sessionId: key });
      if (error?.name === 'AbortError' || error?.phase === 'aborted') {
        return;
      }
      if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
        purgeUnavailableSession(store, key);
        return;
      }
      const resumeRequired = error?.phase === 'slow_client' || error?.resumeRequired === true;
      const transient =
        resumeRequired || error?.phase === 'connect' || error?.phase === 'stream' || error?.name === 'TypeError';
      if (transient) {
        if (perfEnabled) {
          chatPerf.count('chat_watch_interrupted', 1, { sessionId: key });
        }
        return;
      }
      setSessionLoading(store, key, false);
    })
    .finally(() => {
      const runtimeSnapshot = getRuntime(key);
      if (runtimeSnapshot && runtimeSnapshot.watchController === controller) {
        runtimeSnapshot.watchController = null;
        runtimeSnapshot.watchActiveRoundCount = 0;
        runtimeSnapshot.watchRequestId = null;
        clearWatchdog(runtimeSnapshot);
        refreshRuntimeStreamLifecycle(runtimeSnapshot);
        if (chatWatcherSharedState.sessionWatchSessionId === key) {
          chatWatcherSharedState.sessionWatchSessionId = '';
        }
      }
      if (controller.signal.aborted) {
        return;
      }
      const pendingMessage = findPendingAssistantMessage(sessionMessagesRef);
      if (store.activeSessionId === key && (pendingMessage || !desktopMode)) {
        setTimeout(() => startSessionWatcher(store, key), 80);
      }
    });
};

export const chatWsClient = createWsMultiplexer(() => openChatSocket(), {
  idleTimeoutMs: 30000,
  connectTimeoutMs: 10000,
  pingIntervalMs: 20000
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

export const scheduleSlowClientResume = (store, sessionId, message, afterEventId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  const runtime = ensureRuntime(key);
  if (!runtime || runtime.stopRequested) return;
  const hasProjectionSession = Boolean(store?.runtimeProjection?.sessions?.[key]);
  const projectionLastEventId = selectRuntimeLastAppliedEventId(store?.runtimeProjection, key);
  const normalizedAfterEventId = Math.max(
    normalizeStreamEventId(afterEventId) || 0,
    normalizeStreamEventId(message?.stream_event_id) || 0,
    projectionLastEventId,
    hasProjectionSession ? 0 : getRuntimeLastEventId(runtime)
  );
  if (normalizedAfterEventId <= 0) return;
  clearSlowClientResume(runtime);
  runtime.slowClientResumeAfterEventId = normalizedAfterEventId;
  runtime.slowClientResumeTimer = setTimeout(() => {
    runtime.slowClientResumeTimer = null;
    const resumeAfterEventId = Math.max(
      normalizeStreamEventId(runtime.slowClientResumeAfterEventId) || 0,
      normalizedAfterEventId
    );
    runtime.slowClientResumeAfterEventId = 0;
    if (runtime.stopRequested || runtime.sendController || runtime.resumeController) {
      return;
    }
    const currentMessages = getSessionMessages(key) || store.messages;
    const targetMessage =
      message && Array.isArray(currentMessages) && currentMessages.includes(message)
        ? message
        : findPendingAssistantMessage(currentMessages);
    if (targetMessage && !normalizeFlag(targetMessage.stream_incomplete)) {
      return;
    }
    if (chatPerf.enabled()) {
      chatPerf.count('chat_slow_client_auto_resume', 1, { sessionId: key });
    }
    store.resumeStream(key, targetMessage || null, { force: true, afterEventId: resumeAfterEventId });
  }, SLOW_CLIENT_RESUME_DELAY_MS);
};
