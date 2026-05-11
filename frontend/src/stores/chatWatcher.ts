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
import { chatDebugLog, isChatDebugEnabled } from '@/utils/chatDebug';
import { getDesktopToolCallModeForRequest, isDesktopModeEnabled } from '@/config/desktop';
import { resolveAccessToken } from '@/api/requestAuth';
import {
  createChatRuntimeProjection,
  applyChatRuntimeEvent
} from '@/realtime/chat/chatRuntimeReducer';
import {
  selectLegacyMessageStatus,
  selectVisibleMessageProjections,
  selectSessionBusy,
  selectSessionBusyReason,
  selectSessionRuntimeStatus
} from '@/realtime/chat/chatRuntimeSelectors';
import { buildLegacyMessagesReconciledEvent } from '@/realtime/chat/chatRuntimeReplay';
import type { ChatRuntimeProjection } from '@/realtime/chat/chatRuntimeTypes';
import { dedupeAssistantMessages, dedupeAssistantMessagesInPlace } from './chatMessageDedup';
import {
  assistantEntriesShareTurnAnchor,
  buildAssistantMatchEntries,
  buildAssistantMatchEntryMap,
  findAnchoredAssistantContentMatchIndex
} from './chatAssistantMatch';
import {
  clearTrailingPendingAssistantMessages,
  clearSupersededPendingAssistantMessages,
  findPendingAssistantMessage,
  isPendingAssistantMessage,
  stopPendingAssistantMessage
} from './chatPendingMessage';
import {
  captureChatSnapshotScheduleContext,
  resolveChatSnapshotScheduleSource
} from './chatSnapshotScheduler';
import { consumeChatWatchChannelMessage } from './chatWatchChannelMessageRuntime';
import { shouldWatchdogReconcileDrift } from './chatWatchdogRecovery';
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
import {
  mergeProtectedRealtimeMessages,
  upsertProtectedRealtimeMessage
} from './chatRealtimeMessageProtection';
import { useCommandSessionStore } from './commandSessions';
import { hasRetainedMessageConversationContext as hasRetainedConversationContext } from '@/views/messenger/messageConversationRetention';

import { buildWorkflowItem, dismissStaleInquiryPanels, hydrateSessionCommandSessions, normalizeInquiryPanelState, safeJsonParse } from './chatDemoPanels';
import { findAssistantMessageByRound, findAssistantMessageByUserRound } from './chatMessageLookup';
import { applyGoalStreamEvent } from './chatPersist';
import { SLOW_CLIENT_RESUME_DELAY_MS, WATCH_RECONCILE_COOLDOWN_MS, WATCH_RECONCILE_DELAY_MS, WATCH_USER_MESSAGE_DEDUP_MS, abortWatchStream, clearRuntimeInteractiveControllers, clearRuntimeResumeStreamState, clearRuntimeSendStreamState, clearSessionWatcher, clearSlowClientResume, clearWatchdog, insertWatchUserMessage, recoverRuntimeInteractiveControllers, resolveHiddenInternalUserEvent, resolveLastAssistantStreamEventId, resolveLastAssistantTimestampMs, resolveLastStreamEventId, resolveMaxStreamEventId, resolveMaxStreamRound, resolveStreamFlushMsForMessages, resolveWatchdogProfile, setSessionLoading } from './chatRuntimeControls';
import { applySessionRuntimeEvent, applySessionRuntimeSnapshot, buildRuntimeDebugSnapshot, cacheSessionMessages, captureRealtimeWorkflowMutationBaseline, claimRuntimePendingManualCompaction, clearRuntimePendingManualCompaction, clearSessionEventsSnapshot, ensureRuntime, getRuntime, getSessionMessages, handleThreadControlWorkflowEvent, hasKnownSessionInStore, isSessionUnavailableStatus, loadSessionEventsSnapshot, logRealtimeWorkflowMutation, notifySessionSnapshot, protectRealtimeChannelMessage, purgeUnavailableSession, refreshRuntimeStreamLifecycle, resolveChatHttpStatus, resolveSessionContextTokens, resolveSessionKey, resolveSessionMessageArray, sessionDetailPrefetchInFlight, sessionDetailSnapshotCache, sessionDetailWarmState, sessionEventsSnapshotCache, sessionEventsSnapshotInFlight, sessionHistoryState, sessionHydratedMessageVersion, sessionListCache, sessionListCacheInFlight, sessionMessages, sessionProtectedRealtimeMessages, sessionRuntime, sessionSubagentsCache, sessionSubagentsInFlight, syncSessionContextTokens, touchSessionUpdatedAt } from './chatRuntimeState';
import { chatWatcherSharedState } from './chatSharedState';
import { clearAllChatSnapshots, clearScheduledChatSnapshot } from './chatSnapshot';
import { buildMessage, resolveTimestampMs } from './chatStats';
import { assignStreamEventId, getRuntimeLastEventId, normalizeFlag, normalizeStreamEventId, normalizeStreamRound, parseSegmentedDelta, resolveEventRoundNumber, setRuntimeLastEventId, updateRuntimeLastEventId, updateRuntimeRemoteLastEventId } from './chatStreamIds';
import { buildDetail, getSessionWorkflowState, handleApprovalEvent, isTerminalLlmOutputPayload, isTerminalStreamEventType, resolveNormalizedStreamEventType, sessionWorkflowState } from './chatWorkflowHydration';
import { createWorkflowProcessor } from './chatWorkflowProcessor';

export const startSessionWatcher = (store, sessionId) => {
  clearSessionWatcher();
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  if (!hasKnownSessionInStore(store, key)) {
    purgeUnavailableSession(store, key);
    return;
  }
  chatWatcherSharedState.sessionWatchSessionId = key;
  const runtime = ensureRuntime(key);
  if (!runtime) return;
  recoverRuntimeInteractiveControllers(store, key, runtime);
  refreshRuntimeStreamLifecycle(runtime);
  if (runtime.sendController || runtime.resumeController) return;
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
  const workflowState = getSessionWorkflowState(key);
  const roundStates = new Map();
  const completedRounds = new Set();
  const syncWatchActiveRoundCount = () => {
    runtime.watchActiveRoundCount = roundStates.size;
  };
  let maxKnownRound = resolveMaxStreamRound(sessionMessagesRef) || 0;
  const tailEventId =
    resolveLastStreamEventId(sessionMessagesRef) ||
    resolveLastAssistantStreamEventId(sessionMessagesRef) ||
    resolveMaxStreamEventId(sessionMessagesRef) ||
    0;
  const runtimeLastEventId = getRuntimeLastEventId(runtime);
  let lastEventId = runtimeLastEventId > 0 ? runtimeLastEventId : tailEventId;
  const minEventTimestampMs =
    lastEventId > 0 ? null : resolveLastAssistantTimestampMs(sessionMessagesRef);

  const ensureRoundState = (
    roundNumber,
    eventTimestampMs,
    userRoundNumber = null,
    options: { preferFreshRound?: boolean } = {}
  ) => {
    const normalizedRound = normalizeStreamRound(roundNumber);
    if (normalizedRound === null || normalizedRound <= 0) return null;
    maxKnownRound = Math.max(maxKnownRound, normalizedRound);
    if (completedRounds.has(normalizedRound)) return null;
    const existing = roundStates.get(normalizedRound);
    if (existing) return existing;
    const normalizedUserRound = normalizeStreamRound(userRoundNumber);
    const candidateByUserRound = normalizedUserRound
      ? findAssistantMessageByUserRound(sessionMessagesRef, normalizedUserRound)
      : null;
    const pendingCandidate = findPendingAssistantMessage(sessionMessagesRef);
    const pendingRound = normalizeStreamRound(pendingCandidate?.stream_round);
    const pendingCreatedAtMs = resolveTimestampMs(pendingCandidate?.created_at);
    const pendingHasContent =
      typeof pendingCandidate?.content === 'string' && pendingCandidate.content.trim().length > 0;
    const pendingHasWorkflow =
      Array.isArray(pendingCandidate?.workflowItems) && pendingCandidate.workflowItems.length > 0;
    const pendingRoundMismatch =
      pendingRound !== null &&
      pendingRound !== normalizedRound &&
      (normalizedUserRound === null || pendingRound !== normalizedUserRound);
    const preferFreshRound = options?.preferFreshRound === true;
    const pendingNeedsReset =
      Boolean(pendingCandidate) &&
      preferFreshRound &&
      (pendingHasContent || pendingHasWorkflow) &&
      (pendingRound === null || pendingRoundMismatch);
    if (pendingNeedsReset && stopPendingAssistantMessage(pendingCandidate)) {
      const panel = normalizeInquiryPanelState(pendingCandidate?.questionPanel);
      if (panel?.status === 'pending') {
        pendingCandidate.questionPanel = { ...panel, status: 'dismissed' };
      }
    }
    const pendingLooksStale =
      Boolean(pendingCandidate) &&
      Number.isFinite(eventTimestampMs) &&
      Number.isFinite(pendingCreatedAtMs) &&
      eventTimestampMs > Number(pendingCreatedAtMs) + 1500 &&
      (pendingHasContent || pendingHasWorkflow);
    const markPendingManualCompactionCandidate = (message) => {
      if (!message || typeof message !== 'object') return false;
      if (String(message.content || '').trim()) return false;
      if (String(message.reasoning || '').trim()) return false;
      if (Array.isArray(message.workflowItems) && message.workflowItems.length > 0) return false;
      if (!claimRuntimePendingManualCompaction(runtime, key, normalizedRound)) return false;
      message.manual_compaction_marker = true;
      chatDebugLog('chat.compaction.manual', 'watch-marker-applied', {
        sessionId: key,
        round: normalizedRound,
        reused: true,
        createdAt: message.created_at ?? null
      });
      return true;
    };
    const reusablePending =
      pendingCandidate && !pendingNeedsReset && !pendingRoundMismatch && !pendingLooksStale
        ? pendingCandidate
        : null;
    const candidate =
      candidateByUserRound ||
      reusablePending ||
      findAssistantMessageByRound(sessionMessagesRef, normalizedRound);
    if (candidate) {
      const assignedRound = normalizeStreamRound(candidate.stream_round);
      const alreadyTracked = Array.from(roundStates.values()).find((entry) => entry.message === candidate);
      if (alreadyTracked) {
        if (!roundStates.has(normalizedRound)) {
          roundStates.set(normalizedRound, alreadyTracked);
          syncWatchActiveRoundCount();
        }
        if (assignedRound === null || assignedRound !== normalizedRound) {
          candidate.stream_round = normalizedRound;
        }
        if (!candidate.created_at && Number.isFinite(eventTimestampMs)) {
          candidate.created_at = new Date(eventTimestampMs).toISOString();
        }
        markPendingManualCompactionCandidate(candidate);
        candidate.workflowStreaming = true;
        candidate.stream_incomplete = true;
        return alreadyTracked;
      }
      const candidatePending =
        normalizeFlag(candidate.stream_incomplete) || normalizeFlag(candidate.workflowStreaming);
      const candidateHasContent =
        typeof candidate.content === 'string' && candidate.content.trim().length > 0;
      const candidateHasWorkflow =
        Array.isArray(candidate.workflowItems) && candidate.workflowItems.length > 0;
      const placeholderCandidate =
        assignedRound === normalizedRound &&
        !candidatePending &&
        !candidateHasContent &&
        !candidateHasWorkflow;
      if (
        candidatePending ||
        assignedRound === null ||
        (normalizedUserRound !== null && assignedRound === normalizedUserRound) ||
        Boolean(candidateByUserRound) ||
        placeholderCandidate
      ) {
        if (assignedRound === null || assignedRound !== normalizedRound) {
          candidate.stream_round = normalizedRound;
        }
        if (!candidate.created_at && Number.isFinite(eventTimestampMs)) {
          candidate.created_at = new Date(eventTimestampMs).toISOString();
        }
        markPendingManualCompactionCandidate(candidate);
        candidate.workflowStreaming = true;
        candidate.stream_incomplete = true;
        const processor = createWorkflowProcessor(
          candidate,
          workflowState,
          (immediate = false) => notifySessionSnapshot(store, key, sessionMessagesRef, immediate),
          {
            streamFlushMs: resolveStreamFlushMsForMessages(sessionMessagesRef),
            sessionId: key,
            initialContextTokens: resolveSessionContextTokens(store, key),
            onThreadControl: (payload) => handleThreadControlWorkflowEvent(store, payload),
            onContextUsage: (contextTokens, contextTotalTokens) =>
              syncSessionContextTokens(store, key, contextTokens, contextTotalTokens)
          }
        );
        const state = { message: candidate, processor, userInserted: false };
        roundStates.set(normalizedRound, state);
        syncWatchActiveRoundCount();
        return state;
      }
    }
    const createdAt = Number.isFinite(eventTimestampMs)
      ? new Date(eventTimestampMs).toISOString()
      : undefined;
    const assistantMessage = {
      ...buildMessage('assistant', '', createdAt),
      workflowItems: [],
      workflowStreaming: true,
      stream_incomplete: true,
      stream_event_id: lastEventId || 0,
      stream_round: normalizedRound
    };
    if (claimRuntimePendingManualCompaction(runtime, key, normalizedRound)) {
      assistantMessage.manual_compaction_marker = true;
      chatDebugLog('chat.compaction.manual', 'watch-marker-applied', {
        sessionId: key,
        round: normalizedRound,
        reused: false,
        createdAt: assistantMessage.created_at ?? null
      });
    }
    sessionMessagesRef.push(assistantMessage);
    notifySessionSnapshot(store, key, sessionMessagesRef, true);
    const processor = createWorkflowProcessor(
      assistantMessage,
      workflowState,
      (immediate = false) => notifySessionSnapshot(store, key, sessionMessagesRef, immediate),
      {
        streamFlushMs: resolveStreamFlushMsForMessages(sessionMessagesRef),
        sessionId: key,
        initialContextTokens: resolveSessionContextTokens(store, key),
        onThreadControl: (payload) => handleThreadControlWorkflowEvent(store, payload),
        onContextUsage: (contextTokens, contextTotalTokens) =>
          syncSessionContextTokens(store, key, contextTokens, contextTotalTokens)
      }
    );
    const state = { message: assistantMessage, processor, userInserted: false };
    roundStates.set(normalizedRound, state);
    syncWatchActiveRoundCount();
    return state;
  };

  const finalizeRound = (roundNumber, aborted) => {
    const normalizedRound = normalizeStreamRound(roundNumber);
    if (normalizedRound === null) return;
    maxKnownRound = Math.max(maxKnownRound, normalizedRound);
    const state = roundStates.get(normalizedRound);
    if (!state) return;
    if (!aborted) {
      state.message.stream_incomplete = false;
      state.message.workflowStreaming = false;
    }
    state.processor.finalize();
    notifySessionSnapshot(store, key, sessionMessagesRef, true);
    const messageRef = state.message;
    Array.from(roundStates.entries()).forEach(([roundKey, entry]) => {
      if (entry.message === messageRef) {
        roundStates.delete(roundKey);
        completedRounds.add(roundKey);
      }
    });
    syncWatchActiveRoundCount();
  };

  const finalizeAll = (aborted) => {
    Array.from(roundStates.keys()).forEach((round) => finalizeRound(round, aborted));
    syncWatchActiveRoundCount();
    clearRuntimePendingManualCompaction(
      runtime,
      key,
      aborted ? 'watch-aborted' : 'watch-finalized'
    );
  };

  const resolveWatchRoundNumber = (
    eventType,
    payload,
    data,
    isRoundStart
  ) => {
    const directRound = resolveEventRoundNumber(payload, data);
    if (directRound !== null) {
      maxKnownRound = Math.max(maxKnownRound, directRound);
      return directRound;
    }

    const activeRound = Array.from(roundStates.keys()).reduce(
      (maxValue, value) => (value > maxValue ? value : maxValue),
      0
    );
    if (isRoundStart) {
      const nextRound = activeRound > 0 ? activeRound + 1 : Math.max(maxKnownRound + 1, 1);
      maxKnownRound = Math.max(maxKnownRound, nextRound);
      return nextRound;
    }

    const segmented = parseSegmentedDelta(payload, data);
    const hasNonEmptyText = (value: unknown) =>
      typeof value === 'string' && value.trim().length > 0;
    const hasContentDelta =
      Boolean(segmented?.delta) ||
      Boolean(segmented?.reasoningDelta) ||
      hasNonEmptyText(data?.delta) ||
      hasNonEmptyText(data?.content) ||
      hasNonEmptyText(data?.answer) ||
      hasNonEmptyText(data?.message) ||
      hasNonEmptyText(payload?.message);
    const normalizedEventType = String(eventType || '').trim().toLowerCase();
    const eventHasWorkflowHint =
      hasContentDelta ||
      normalizedEventType === 'final' ||
      normalizedEventType === 'error' ||
      normalizedEventType === 'turn_terminal' ||
      normalizedEventType === 'queue_enter' ||
      normalizedEventType === 'queue_update' ||
      normalizedEventType === 'queue_start' ||
      normalizedEventType === 'queue_finish' ||
      normalizedEventType === 'queue_fail' ||
      normalizedEventType === 'received' ||
      normalizedEventType === 'round_start' ||
      normalizedEventType === 'progress' ||
      normalizedEventType === 'message' ||
      normalizedEventType === 'delta' ||
      normalizedEventType === 'think_delta' ||
      normalizedEventType === 'reasoning_delta' ||
      normalizedEventType === 'llm_output' ||
      normalizedEventType === 'tool_call' ||
      normalizedEventType === 'tool_result' ||
      normalizedEventType === 'tool_output' ||
      normalizedEventType === 'team_start' ||
      normalizedEventType === 'team_task_dispatch' ||
      normalizedEventType === 'team_task_update' ||
      normalizedEventType === 'team_task_result' ||
      normalizedEventType === 'team_merge' ||
      normalizedEventType === 'team_progress' ||
      normalizedEventType === 'team_finish' ||
      normalizedEventType === 'team_error' ||
      normalizedEventType.startsWith('subagent_');
    if (!eventHasWorkflowHint) {
      return null;
    }

    if (activeRound > 0) {
      maxKnownRound = Math.max(maxKnownRound, activeRound);
      return activeRound;
    }
    const fallbackRound = Math.max(maxKnownRound, 1);
    maxKnownRound = Math.max(maxKnownRound, fallbackRound);
    return fallbackRound;
  };

  const isWatchTerminalEventType = (normalizedEventType) =>
    isTerminalStreamEventType(normalizedEventType);

  const isWatchWorkflowEventType = (normalizedEventType) =>
    isWatchTerminalEventType(normalizedEventType) ||
    normalizedEventType === 'queue_enter' ||
    normalizedEventType === 'queue_update' ||
    normalizedEventType === 'queue_start' ||
    normalizedEventType === 'queue_finish' ||
    normalizedEventType === 'received' ||
    normalizedEventType === 'round_start' ||
    normalizedEventType === 'progress' ||
    normalizedEventType === 'message' ||
    normalizedEventType === 'delta' ||
    normalizedEventType === 'think_delta' ||
    normalizedEventType === 'reasoning_delta' ||
    normalizedEventType === 'llm_output' ||
    normalizedEventType === 'tool_call' ||
    normalizedEventType === 'tool_result' ||
    normalizedEventType === 'tool_output' ||
    normalizedEventType === 'team_start' ||
    normalizedEventType === 'team_task_dispatch' ||
    normalizedEventType === 'team_task_update' ||
    normalizedEventType === 'team_task_result' ||
    normalizedEventType === 'team_merge' ||
    normalizedEventType === 'team_progress' ||
    normalizedEventType === 'team_finish' ||
    normalizedEventType === 'team_error' ||
    normalizedEventType.startsWith('subagent_');

  const isWatchSidebandEventType = (normalizedEventType) =>
    normalizedEventType === 'channel_message' || normalizedEventType.startsWith('team_');

  const extractWatchUserContent = (normalizedEventType, payload, data) => {
    const candidates = [
      data?.question,
      payload?.question,
      data?.user_message,
      payload?.user_message,
      data?.user_content,
      payload?.user_content,
      data?.input,
      payload?.input,
      data?.prompt,
      payload?.prompt
    ];
    if (normalizedEventType === 'round_start' || normalizedEventType === 'received') {
      candidates.push(data?.message, payload?.message);
    }
    for (const item of candidates) {
      if (typeof item === 'string' && item.trim()) {
        return item.trim();
      }
    }
    return '';
  };

  const markWatchdogEvent = () => {
    runtime.watchLastEventAt = Date.now();
  };

  // Reconcile from server when watch events appear out-of-sync with stream state.
  const scheduleWatchReconcile = (delayMs = WATCH_RECONCILE_DELAY_MS) => {
    if (controller.signal.aborted) return;
    if (store.activeSessionId !== key) return;
    if (!hasKnownSessionInStore(store, key)) return;
    recoverRuntimeInteractiveControllers(store, key, runtime, {
      localLastEventId: lastEventId
    });
    if (runtime.sendController || runtime.resumeController) return;
    const now = Date.now();
    const nextAllowedAt = Number(runtime.watchReconcileAt) || 0;
    if (nextAllowedAt > now) {
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
      if (runtime.sendController || runtime.resumeController) return;
      void store.loadSessionDetail(key, { preserveWatcher: true }).catch(() => {});
    }, Math.max(0, Number(delayMs) || 0));
  };

  const startWatchdog = () => {
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
      recoverRuntimeInteractiveControllers(store, key, runtime, {
        localLastEventId: lastEventId
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
        const running = payload?.running;
        const remoteLastEventId = Number(payload?.last_event_id ?? payload?.lastEventId);
        updateRuntimeRemoteLastEventId(runtime, remoteLastEventId);
        recoverRuntimeInteractiveControllers(store, key, runtime, {
          remoteRunning: running,
          remoteLastEventId,
          localLastEventId: lastEventId
        });
        const pendingMessage = findPendingAssistantMessage(sessionMessagesRef);
        const shouldReconcileRemoteDrift = shouldWatchdogReconcileDrift({
          remoteLastEventId,
          localLastEventId: lastEventId,
          hasPendingMessage: Boolean(pendingMessage)
        });
        if (
          pendingMessage &&
          Number.isFinite(remoteLastEventId) &&
          remoteLastEventId > lastEventId
        ) {
          if (perfEnabled) {
            chatPerf.count('chat_watchdog_resume', 1, { sessionId: key });
          }
          store.resumeStream(key, pendingMessage, {
            force: true,
            afterEventId: lastEventId
          });
          return;
        }
        if (shouldReconcileRemoteDrift) {
          scheduleWatchReconcile(running === false ? 0 : WATCH_RECONCILE_DELAY_MS);
        }
        if (running === false) {
          clearRuntimeInteractiveControllers(runtime, { abort: false });
          clearSupersededPendingAssistantMessages(sessionMessagesRef);
          clearTrailingPendingAssistantMessages(sessionMessagesRef);
          setSessionLoading(store, key, false);
          notifySessionSnapshot(store, key, sessionMessagesRef, true);
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

  const tryFinalizeWatchRound = (roundHint) => {
    const normalizedRound = normalizeStreamRound(roundHint);
    if (normalizedRound === null || !roundStates.has(normalizedRound)) {
      return false;
    }
    finalizeRound(normalizedRound, false);
    return true;
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
    if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
      const normalizedEventId = normalizeStreamEventId(eventId);
      if (normalizedEventId !== null) {
        updateRuntimeRemoteLastEventId(runtime, normalizedEventId);
      }
      applySessionRuntimeEvent(store, key, data ?? payload, normalizedEventType);
      return;
    }
    if (
      (runtime.sendController || runtime.resumeController) &&
      !isWatchSidebandEventType(normalizedEventType)
    ) {
      return;
    }
    if (perfEnabled) {
      chatPerf.count('chat_watch_event', 1, { eventType: normalizedEventType || eventType, sessionId: key });
    }
    if (normalizedEventType === 'heartbeat' || normalizedEventType === 'ping') {
      return;
    }
    handleApprovalEvent(store, normalizedEventType || eventType, data, requestId, key);
    if (normalizedEventType === 'slow_client' && !data) {
      return;
    }
    const stage = data?.stage ?? payload?.stage;
    const eventTimestampMs = resolveTimestampMs(payload?.timestamp ?? data?.timestamp);
    const normalizedEventId = normalizeStreamEventId(eventId);
    if (normalizedEventType === 'channel_message') {
      const channelRole = String(data?.role ?? payload?.role ?? '').trim().toLowerCase();
      const channelContent = String(data?.content ?? payload?.content ?? '').trim();
      const result = consumeChatWatchChannelMessage({
        messages: sessionMessagesRef,
        lastEventId,
        eventId,
        eventTimestampMs,
        payload,
        data,
        normalizeEventId: normalizeStreamEventId,
        buildMessage,
        assignStreamEventId,
        insertWatchUserMessage: (content, timestampMs, anchor, options) =>
          insertWatchUserMessage(store, key, sessionMessagesRef, content, timestampMs, anchor, options),
        clearSupersededPendingAssistantMessages,
        dismissStaleInquiryPanels,
        touchUpdatedAt: (timestamp) => touchSessionUpdatedAt(store, key, timestamp),
        notifySnapshot: (immediate = true) =>
          notifySessionSnapshot(store, key, sessionMessagesRef, immediate),
        hiddenInternalUser: resolveHiddenInternalUserEvent(payload, data),
        dedupeAssistantWindowMs: WATCH_USER_MESSAGE_DEDUP_MS
      });
      if (result.handled) {
        if (result.lastEventId > lastEventId) {
          updateRuntimeLastEventId(runtime, result.lastEventId);
          updateRuntimeRemoteLastEventId(runtime, result.lastEventId);
          lastEventId = result.lastEventId;
        }
        if (
          result.mutated &&
          normalizedEventId !== null &&
          (channelRole === 'user' || channelRole === 'assistant') &&
          channelContent
        ) {
          protectRealtimeChannelMessage(
            key,
            sessionMessagesRef,
            normalizedEventId,
            channelRole,
            channelContent,
            eventTimestampMs,
            resolveHiddenInternalUserEvent(payload, data)
          );
        }
        return;
      }
    }
    const userRoundNumber = normalizeStreamRound(data?.user_round ?? payload?.user_round);
    const directRoundNumber = resolveEventRoundNumber(payload, data);
    const isRoundStart =
      normalizedEventType === 'round_start' ||
      normalizedEventType === 'received' ||
      (normalizedEventType === 'progress' && stage === 'start');
    const llmOutputTerminal =
      normalizedEventType === 'llm_output' && isTerminalLlmOutputPayload(payload, data);
    if (normalizedEventId !== null) {
      if (normalizedEventId <= lastEventId) {
        const latestAssistantTimestamp = resolveLastAssistantTimestampMs(sessionMessagesRef);
        const hasPendingAssistant = Boolean(findPendingAssistantMessage(sessionMessagesRef));
        const highestActiveRound = Array.from(roundStates.keys()).reduce(
          (maxValue, value) => (value > maxValue ? value : maxValue),
          0
        );
        const knownRoundCeiling = Math.max(maxKnownRound, highestActiveRound);
        const directRoundAdvanced =
          directRoundNumber !== null && directRoundNumber > knownRoundCeiling;
        const userRoundAdvanced =
          userRoundNumber !== null && userRoundNumber > knownRoundCeiling;
        const timestampLooksNew =
          !Number.isFinite(eventTimestampMs) ||
          !Number.isFinite(latestAssistantTimestamp) ||
          eventTimestampMs > Number(latestAssistantTimestamp) + 200;
        const canResetByStart =
          isRoundStart &&
          roundStates.size === 0 &&
          !hasPendingAssistant &&
          timestampLooksNew;
        const canResetByRoundHint =
          roundStates.size === 0 &&
          !hasPendingAssistant &&
          timestampLooksNew &&
          isWatchWorkflowEventType(normalizedEventType) &&
          (directRoundAdvanced || userRoundAdvanced);
        if (!canResetByStart && !canResetByRoundHint) {
          if (isWatchTerminalEventType(normalizedEventType) || llmOutputTerminal) {
            const finalizedByRound =
              tryFinalizeWatchRound(directRoundNumber) ||
              tryFinalizeWatchRound(userRoundNumber) ||
              (roundStates.size === 1 && tryFinalizeWatchRound(Array.from(roundStates.keys())[0]));
            if (finalizedByRound) {
              setSessionLoading(store, key, false);
              if (perfEnabled) {
                chatPerf.count('chat_watch_terminal', 1, {
                  eventType: normalizedEventType || eventType,
                  sessionId: key
                });
              }
              return;
            }
            if (roundStates.size === 0) {
              chatDebugLog('chat.watch', 'ignore-duplicate-terminal', {
                sessionId: key,
                eventType: normalizedEventType || eventType,
                eventId: normalizedEventId,
                lastEventId,
                directRoundNumber,
                userRoundNumber,
                runtime: buildRuntimeDebugSnapshot(runtime)
              });
              if (perfEnabled) {
                chatPerf.count('chat_watch_duplicate_terminal', 1, {
                  eventType: normalizedEventType || eventType,
                  sessionId: key
                });
              }
              return;
            }
          }
          if (isWatchWorkflowEventType(normalizedEventType)) {
            scheduleWatchReconcile();
          }
          return;
        }
        setRuntimeLastEventId(runtime, normalizedEventId);
        updateRuntimeRemoteLastEventId(runtime, normalizedEventId);
      } else {
        updateRuntimeLastEventId(runtime, normalizedEventId);
        updateRuntimeRemoteLastEventId(runtime, normalizedEventId);
      }
      lastEventId = normalizedEventId;
    }
    if (
      normalizedEventId === null &&
      Number.isFinite(minEventTimestampMs) &&
      Number.isFinite(eventTimestampMs) &&
      eventTimestampMs <= minEventTimestampMs
    ) {
      return;
    }
    const roundNumber = resolveWatchRoundNumber(eventType, payload, data, isRoundStart);
    const userContent = extractWatchUserContent(normalizedEventType, payload, data);
    const hiddenInternalUser = resolveHiddenInternalUserEvent(payload, data);
    const shouldPreinsertUserMessage =
      (isRoundStart || normalizedEventType === 'received') && Boolean(userContent);
    if (shouldPreinsertUserMessage) {
      // Insert the new user turn before allocating the assistant round so stale pending
      // assistant content from the previous turn cannot be reused as the current response shell.
      insertWatchUserMessage(store, key, sessionMessagesRef, userContent, eventTimestampMs, null, {
        hiddenInternal: hiddenInternalUser
      });
    }
    const state = ensureRoundState(roundNumber, eventTimestampMs, userRoundNumber, {
      preferFreshRound: isRoundStart || Boolean(userContent)
    });
    if (state && shouldPreinsertUserMessage) {
      state.userInserted = true;
    } else if (state && (isRoundStart || normalizedEventType === 'received')) {
      if (!state.userInserted && userContent) {
        insertWatchUserMessage(
          store,
          key,
          sessionMessagesRef,
          userContent,
          eventTimestampMs,
          state.message,
          { hiddenInternal: hiddenInternalUser }
        );
        state.userInserted = true;
      }
    }
    if (!state) {
      if (isWatchWorkflowEventType(normalizedEventType)) {
        scheduleWatchReconcile();
      }
      return;
    }
    state.message.workflowStreaming = true;
    state.message.stream_incomplete = true;
    assignStreamEventId(state.message, eventId);
    const mutationBaseline = captureRealtimeWorkflowMutationBaseline(
      state.message,
      sessionMessagesRef
    );
    if (perfEnabled) {
      const start = performance.now();
      state.processor.handleEvent(normalizedEventType || eventType, dataText);
      chatPerf.recordDuration('chat_watch_event_handle', performance.now() - start, {
        eventType: normalizedEventType || eventType,
        sessionId: key
      });
    } else {
      state.processor.handleEvent(normalizedEventType || eventType, dataText);
    }
    logRealtimeWorkflowMutation({
      phase: 'watch',
      sessionId: key,
      eventType: normalizedEventType || eventType,
      eventId,
      roundNumber,
      userRoundNumber,
      message: state.message,
      messages: sessionMessagesRef,
      before: mutationBaseline
    });
    if (isWatchTerminalEventType(normalizedEventType) || llmOutputTerminal) {
      const finalized =
        tryFinalizeWatchRound(roundNumber) ||
        (roundStates.size === 1 && tryFinalizeWatchRound(Array.from(roundStates.keys())[0]));
      if (finalized || roundStates.size === 0) {
        setSessionLoading(store, key, false);
      } else {
        scheduleWatchReconcile();
      }
      if (perfEnabled) {
        chatPerf.count('chat_watch_terminal', 1, {
          eventType: normalizedEventType || eventType,
          sessionId: key
        });
      }
    }
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
        finalizeAll(true);
        return;
      }
      if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
        purgeUnavailableSession(store, key);
        finalizeAll(false);
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
      finalizeAll(false);
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
      if (store.activeSessionId === key && (pendingMessage || !controller.signal.aborted)) {
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
  clearRuntimeResumeStreamState(runtime, { abort: true });
  refreshRuntimeStreamLifecycle(runtime);
};

export const abortSendStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  clearSlowClientResume(runtime);
  clearRuntimeSendStreamState(runtime, { abort: true });
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
        toolName: '上下文压缩',
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
          toolName: '上下文压缩',
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
          toolName: '上下文压缩',
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
  sessionWorkflowState.clear();
  clearScheduledChatSnapshot();
  clearAllChatSnapshots();
};

export const resolveLegacyMessageRuntimeStatusFromStore = (store, sessionId, message) => {
  const projection = store?.runtimeProjection as ChatRuntimeProjection | undefined;
  const key = resolveSessionKey(sessionId || store?.activeSessionId);
  if (!projection || !key || !message) return null;
  return selectLegacyMessageStatus(projection, key, message);
};

export const resolveProjectedVisibleMessagesFromStore = (store, sessionId) => {
  const projection = store?.runtimeProjection as ChatRuntimeProjection | undefined;
  const key = resolveSessionKey(sessionId || store?.activeSessionId);
  const sourceMessages = resolveSessionKey(store?.activeSessionId) === key
    ? store?.messages
    : getSessionMessages(key);
  if (!projection || !key || !Array.isArray(sourceMessages)) {
    return Array.isArray(sourceMessages) ? sourceMessages : [];
  }
  const projected = selectVisibleMessageProjections(projection, key);
  if (!projected.length) {
    return sourceMessages;
  }
  const byRaw = new Map();
  const used = new Set();
  sourceMessages.forEach((message) => {
    if (message && typeof message === 'object') {
      byRaw.set(message, message);
    }
  });
  const ordered = projected
    .map((item) => byRaw.get(item.raw))
    .filter((message) => {
      if (!message || used.has(message)) return false;
      used.add(message);
      return true;
    });
  if (!ordered.length) {
    return sourceMessages;
  }
  sourceMessages.forEach((message) => {
    if (!used.has(message)) {
      ordered.push(message);
    }
  });
  return ordered;
};

export const scheduleSlowClientResume = (store, sessionId, message, afterEventId) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !message) return;
  const runtime = ensureRuntime(key);
  if (!runtime || runtime.stopRequested) return;
  const normalizedAfterEventId = Math.max(
    normalizeStreamEventId(afterEventId) || 0,
    normalizeStreamEventId(message.stream_event_id) || 0,
    getRuntimeLastEventId(runtime)
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
      Array.isArray(currentMessages) && currentMessages.includes(message)
        ? message
        : findPendingAssistantMessage(currentMessages);
    if (!targetMessage || !normalizeFlag(targetMessage.stream_incomplete)) {
      return;
    }
    if (chatPerf.enabled()) {
      chatPerf.count('chat_slow_client_auto_resume', 1, { sessionId: key });
    }
    store.resumeStream(key, targetMessage, { force: true, afterEventId: resumeAfterEventId });
  }, SLOW_CLIENT_RESUME_DELAY_MS);
};
