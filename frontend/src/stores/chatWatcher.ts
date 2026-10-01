import { advanceThreadChangeCursor, threadChangeCursor } from './chatThreadCursor';
import { selectVisibleMessageProjections } from '@/realtime/chat/chatRuntimeSelectors';
import {
  applyChatThreadServerEvent,
  ensureChatThreadRuntime,
  getChatThreadState,
  isChatChangeStreamServerSupported,
  isChatThreadV2Session,
  registerChatThreadSnapshotLoader,
  setChatChangeStreamServerSupported
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
import { SLOW_CLIENT_RESUME_DELAY_MS, WATCH_RECONCILE_COOLDOWN_MS, WATCH_RECONCILE_DELAY_MS, abortWatchStream, clearRuntimeInteractiveControllers, clearRuntimeResumeStreamState, clearRuntimeSendStreamState, clearSessionWatcher, clearSlowClientResume, clearWatchdog, recoverRuntimeInteractiveControllers, resolveLastAssistantStreamEventId, resolveLastStreamEventId, resolveMaxStreamEventId, resolveWatchdogProfile, setSessionLoading } from './chatRuntimeControls';
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

  const pendingThreadChanges = new Map<string, Record<string, unknown>>();
  let threadReconcileTimer: ReturnType<typeof setTimeout> | null = null;
  let threadResumeTimer: ReturnType<typeof setTimeout> | null = null;
  let threadReconcileInFlight: Promise<void> | null = null;
  let pendingThreadStatus: Record<string, any> | null = null;
  let threadRecoveryFailed = false;

  const mergeThreadTurnItems = (turn: Record<string, unknown> | null | undefined) => {
    const rawItems = Array.isArray(turn?.items) ? turn.items : [];
    if (!rawItems.length) return false;
    const current = resolveSessionMessageArray(store, key, sessionMessagesRef);
    if (!Array.isArray(current)) return false;
    const projected = new Map(selectVisibleMessageProjections(store.runtimeProjection, key)
      .map(message => [message.id, message]));
    current.forEach(message => {
      const live = projected.get(String(message.message_id ?? message.id ?? ''));
      if (live?.role === 'assistant') {
        Object.assign(message, { content: live.content, reasoning: live.reasoning,
          status: live.status, final: live.final, failed: live.failed, cancelled: live.cancelled });
      }
    });
    const byItemId = new Map<string, Record<string, any>>();
    current.forEach((message) => {
      const itemId = String(message?.item_id ?? '').trim();
      if (itemId) byItemId.set(itemId, message);
    });
    let changed = false;
    const turnStatus = String(turn?.status ?? '').trim().toLowerCase();
    for (const rawItem of rawItems) {
      if (!rawItem || typeof rawItem !== 'object') continue;
      const item = rawItem as Record<string, any>;
      const payload = item.payload && typeof item.payload === 'object' && !Array.isArray(item.payload)
        ? item.payload
        : {};
      const kind = String(item.kind ?? payload.kind ?? '').trim().toLowerCase();
      const role = String(payload.role ?? item.role ??
        (kind === 'user_message' ? 'user' : kind === 'assistant_message' ? 'assistant' : '')).trim();
      if (role !== 'user' && role !== 'assistant') continue;
      const itemId = String(item.item_id ?? payload.item_id ?? '').trim();
      if (!itemId) continue;
      const storedStatus = String(item.status ?? payload.status ?? '').trim().toLowerCase();
      const itemStatus = payload.meta?.type === 'session_cancelled' ? 'cancelled'
        : ['running', 'waiting_input', 'queued'].includes(storedStatus) &&
          ['completed', 'failed', 'cancelled', 'interrupted'].includes(turnStatus) ? turnStatus : storedStatus;
      const renderStatus = itemStatus === 'failed' ? 'failed'
        : itemStatus === 'cancelled' || itemStatus === 'canceled' || itemStatus === 'interrupted' ? 'cancelled'
          : itemStatus === 'queued' ? 'queued'
            : itemStatus === 'running' || itemStatus === 'waiting_input' ? 'streaming'
              : itemStatus === 'completed' ? 'final' : undefined;
      const revision = Number(item.revision ?? payload.revision ?? 0);
      const existing = byItemId.get(itemId);
      const existingRevision = Number(existing?.revision ?? existing?.thread_item_revision ?? 0);
      if (existing && Number.isFinite(revision) && revision > 0 && existingRevision >= revision) continue;
      const next = {
        ...payload,
        role,
        item_id: itemId,
        turn_id: String(item.turn_id ?? payload.turn_id ?? turn?.turn_id ?? '').trim(),
        kind,
        created_seq: item.created_seq ?? payload.created_seq,
        stats: payload.stats ?? payload.meta?.message_stats,
        ...(renderStatus ? { status: renderStatus } : {}),
        ...(role === 'assistant' ? {
          final: renderStatus === 'final', failed: renderStatus === 'failed',
          cancelled: renderStatus === 'cancelled',
          stream_incomplete: renderStatus === 'streaming', workflowStreaming: false,
          reasoningStreaming: false
        } : {}),
        turn_index: existing?.turn_index ?? current.length + 1,
        visibility: item.visibility ?? payload.visibility ?? 'user',
        revision: Number.isFinite(revision) && revision > 0 ? revision : existingRevision + 1,
        thread_item_revision: Number.isFinite(revision) && revision > 0 ? revision : existingRevision + 1,
        user_turn_id: Number(payload.user_round ?? turn?.user_turn_index) > 0
          ? `user-turn:${key}:round:${Number(payload.user_round ?? turn?.user_turn_index)}`
          : String(item.turn_id ?? payload.turn_id ?? turn?.turn_id ?? '').trim(),
        user_turn_index: Number(payload.user_round ?? turn?.user_turn_index) || undefined,
        model_turn_id: role === 'assistant' && Number(payload.model_round) > 0
          ? `model-turn:${key}:user:${Number(payload.user_round ?? turn?.user_turn_index)}:model:${Number(payload.model_round)}`
          : payload.model_turn_id,
        message_id: String(payload.message_id ?? payload.id ?? `item:${itemId}`),
        ...(payload.meta?.type === 'manual_compaction_marker' ? { manual_compaction_marker: true } : {}),
        content: typeof payload.content === 'string' ? payload.content : String(payload.content ?? '')
      };
      if (existing) {
        if (renderStatus === 'streaming') {
          if (String(existing.content || '').length > next.content.length) next.content = existing.content;
          if (String(existing.reasoning || '').length > String(next.reasoning || '').length) next.reasoning = existing.reasoning;
        }
        Object.assign(existing, next);
      } else {
        current.push(next);
        byItemId.set(itemId, next);
      }
      changed = true;
    }
    const durableTurnId = String(turn?.turn_id ?? '').trim();
    // Turn activity never revives completed model messages. In particular an
    // identity-less page must not match every legacy message's empty turn id.
    if (durableTurnId && ['completed', 'failed', 'cancelled', 'interrupted'].includes(turnStatus)) {
      current.forEach((message) => {
        if (message?.role !== 'assistant') return;
        const messageTurnId = String(message?.turn_id ?? message?.user_turn_id ?? '').trim();
        if (messageTurnId !== durableTurnId) return;
        if (['final', 'completed', 'cancelled', 'failed'].includes(String(message.status || ''))) return;
        const nextStatus = turnStatus === 'failed' ? 'failed'
          : turnStatus === 'cancelled' || turnStatus === 'interrupted' ? 'cancelled'
            : turnStatus === 'completed' ? 'final' : null;
        if (nextStatus && message.status !== nextStatus) {
          message.status = nextStatus;
          message.failed = nextStatus === 'failed';
          message.cancelled = nextStatus === 'cancelled';
          message.final = nextStatus === 'final';
          message.stream_incomplete = false;
          message.workflowStreaming = false;
          message.reasoningStreaming = false;
          changed = true;
        }
      });
    }
    if (changed) {
      cacheSessionMessages(key, current);
      syncChatRuntimeProjectionFromSnapshot(store, key, current, {
        immediate: true, loading: false,
        running: isThreadRuntimeBusy(runtime?.threadStatus) ||
          current.some(message => message.role === 'assistant' &&
            ['streaming', 'tooling', 'waiting_first_output'].includes(message.status))
      });
      notifySessionSnapshot(store, key, current, true);
    }
    if (Array.isArray(turn?.events) && turn.events.length) {
      applyCanonicalSessionEventsSnapshot(store, key, { events: turn.events }, {
        phase: 'watch', includeRuntime: false
      });
    }
    return changed;
  };

  const flushThreadChanges = async () => {
    threadReconcileTimer = null;
    const changes = Array.from(pendingThreadChanges.values());
    pendingThreadChanges.clear();
    if (!changes.length || controller.signal.aborted) return;
    const changesByTurn = new Map<string, Set<string>>();
    changes.forEach((change) => {
      const turnId = String(change.turn_id ?? '').trim();
      if (!turnId || change.change_type === 'text_block' || change.change_type === 'cursor') return;
      const items = changesByTurn.get(turnId) || new Set<string>();
      const itemId = String(change.item_id ?? '').trim();
      if (itemId) items.add(itemId);
      changesByTurn.set(turnId, items);
    });
    const turnIds = Array.from(changesByTurn.keys());
    const task = (async () => {
      for (const turnId of turnIds) {
        const targetItems = changesByTurn.get(turnId) || new Set<string>();
        let after = -1;
        let page = 0;
        do {
          const response = await getThreadLogTurn(key, turnId, { item_after: after, limit: 100 }, { signal: controller.signal });
          const turn = response?.data?.data?.turn;
          if (controller.signal.aborted) return;
          mergeThreadTurnItems(turn);
          const items = Array.isArray(turn?.items) ? turn.items : [];
          items.forEach(item => targetItems.delete(String(item?.item_id ?? '').trim()));
          const found = targetItems.size === 0;
          if (turn?.has_more !== true) {
            if (!found) throw new Error('Thread items missing from recovery page');
            break;
          }
          const next = Number(turn?.next_after);
          if (!Number.isFinite(next) || next <= after) break;
          after = next;
          page += 1;
        } while (page < 20 && !controller.signal.aborted);
        if (targetItems.size > 0 || page >= 20) throw new Error('Thread recovery requires a fresh snapshot');
      }
    })();
    threadReconcileInFlight = task;
    try {
      await task;
      if (!controller.signal.aborted) {
        advanceThreadChangeCursor(runtime, Math.max(...changes.map(change => Number(change.cursor) || 0)));
        if (!pendingThreadChanges.size && pendingThreadStatus) {
          applyRecoveredThreadStatus(pendingThreadStatus);
          pendingThreadStatus = null;
        }
      }
    } catch {
      if (!controller.signal.aborted) {
        threadRecoveryFailed = true;
        scheduleWatchReconcile(0);
      }
    } finally {
      if (threadReconcileInFlight === task) threadReconcileInFlight = null;
      if (pendingThreadChanges.size > 0 && !controller.signal.aborted && !threadReconcileTimer && !threadRecoveryFailed) {
        threadReconcileTimer = setTimeout(() => { void flushThreadChanges(); }, 0);
      }
    }
  };

  const scheduleThreadLogReconcile = (data: unknown) => {
    if (threadRecoveryFailed) return;
    const change = data && typeof data === 'object' ? data as Record<string, unknown> : {};
    const turnId = String(change.turn_id ?? '').trim();
    const itemId = String(change.item_id ?? '').trim();
    if (!turnId && change.change_type !== 'cursor') {
      scheduleWatchReconcile(0);
      return;
    }
    if (pendingThreadChanges.size >= 500) {
      threadRecoveryFailed = true;
      pendingThreadChanges.clear();
      scheduleWatchReconcile(0);
      return;
    }
    pendingThreadChanges.set(`${turnId}:${itemId}:${change.change_type}`, change);
    if (threadReconcileTimer || threadReconcileInFlight) return;
    threadReconcileTimer = setTimeout(() => { void flushThreadChanges(); }, 0);
  };

  controller.signal.addEventListener('abort', () => {
    if (threadReconcileTimer) clearTimeout(threadReconcileTimer);
    if (threadResumeTimer) clearTimeout(threadResumeTimer);
    pendingThreadChanges.clear();
  }, { once: true });

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
      void store.loadSessionDetail(key, { preserveWatcher: true, startWatcherAfterHydration: false })
        .then(() => { if (!controller.signal.aborted) startSessionWatcher(store, key); })
        .catch(() => {});
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
      if (useChangeStream) {
        // M2-A: the v2 watchdog is a liveness probe only. It never loads or
        // overlays snapshots and never reconciles cursors — the reducer's
        // lastSeq owns the durable cursor. A running session silent past the
        // threshold means the connection is dead: force a reconnect; the new
        // watch resumes from lastSeq without any full reload.
        const lastEventAt = Number(runtime.watchLastEventAt) || 0;
        const livenessIdleMs = Math.max(Number(profile.idleMs) || 0, WATCHDOG_V2_LIVENESS_IDLE_MS);
        const running = isThreadRuntimeBusy(runtime?.threadStatus) ||
          hasRunningAssistantMessage(sessionMessagesRef);
        if (running && lastEventAt && Date.now() - lastEventAt >= livenessIdleMs &&
            !runtime.sendController && !runtime.resumeController && !runtime.watchdogBusy) {
          if (chatPerf.enabled()) {
            chatPerf.count('chat_watch_v2_liveness_reconnect', 1, { sessionId: key });
          }
          controller.abort();
          startSessionWatcher(store, key);
          return;
        }
        scheduleNext(profile.intervalMs);
        return;
      }
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
        if (controller.signal.aborted || runtime.watchController !== controller ||
            !isChatSnapshotCurrent(runtime, payload)) return;
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
        const remoteCursor = Number(payload?.thread_change_cursor);
        const shouldReconcileRemoteDrift =
          Number.isSafeInteger(remoteCursor) && remoteCursor > threadChangeCursor(runtime);
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
          if (chatPerf.enabled()) {
            chatPerf.count('chat_watchdog_idle_complete', 1, { sessionId: key });
          }
        } else if (chatPerf.enabled()) {
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

  const applyRecoveredThreadStatus = (data: Record<string, any>) => {
    applyCanonicalStreamRuntimeEvent(store, key, 'thread_status', { data }, null, {
      requestId, phase: 'watch'
    });
    applySessionRuntimeEvent(store, key, data, 'thread_status');
  };

  // v2 recovery (plan §5 M2-A/M2-C): the reducer owns the durable cursor, so
  // overflow, gap overflow and a freshly applied atomic snapshot all recover
  // by restarting the watch — its after_event_id is already state.lastSeq.
  // No full reload, and the runtime's cooldown collapses duplicate bursts.
  const resumeWatchFromLastSeq = () => {
    if (controller.signal.aborted || threadResumeTimer) return;
    threadResumeTimer = setTimeout(() => {
      threadResumeTimer = null;
      if (controller.signal.aborted) return;
      if (runtime.watchController !== controller) return; // already replaced
      startSessionWatcher(store, key);
    }, 0);
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
    if (
      useChangeStream &&
      applyChatThreadServerEvent(store, key, normalizedEventType || eventType, payload, {
        onSnapshotRequired: () => {
          // Runtime could not rebuild from the atomic snapshot (loader failed
          // or stale): keep the legacy full-reload fallback path.
          threadRecoveryFailed = true;
          void store.loadSessionDetail(key, { preserveWatcher: true, startWatcherAfterHydration: false })
            .then(() => { if (!controller.signal.aborted) startSessionWatcher(store, key); })
            .catch(() => scheduleWatchReconcile(0));
        },
        onSnapshotApplied: () => resumeWatchFromLastSeq(),
        onOverflow: () => resumeWatchFromLastSeq(),
        onGapOverflow: () => resumeWatchFromLastSeq()
      })
    ) {
      return;
    }
    if (normalizedEventType === 'thread_change') {
      scheduleThreadLogReconcile(data);
      return;
    }
    if (normalizedEventType === 'thread_snapshot_required') {
      threadRecoveryFailed = true;
      void store.loadSessionDetail(key, { preserveWatcher: true, startWatcherAfterHydration: false })
        .then(() => { if (!controller.signal.aborted) startSessionWatcher(store, key); })
        .catch(() => scheduleWatchReconcile(0));
      return;
    }
    if (normalizedEventType === 'thread_status' && data?.recovery === true) {
      if (pendingThreadChanges.size || threadReconcileInFlight || threadRecoveryFailed) {
        pendingThreadStatus = data;
      } else {
        applyRecoveredThreadStatus(data);
      }
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
    if (normalizedEventType === 'thread_item_block') return;
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
    if (chatPerf.enabled()) {
      chatPerf.count('chat_watch_event', 1, { eventType: normalizedEventType || eventType, sessionId: key });
    }
    handleApprovalEvent(store, normalizedEventType || eventType, data, requestId, key);
    const projectionTerminal =
      isTerminalStreamEventType(normalizedEventType) ||
      (normalizedEventType === 'llm_output' && isTerminalLlmOutputPayload(payload, data));
    if (projectionTerminal) {
      setSessionLoading(store, key, false);
      if (chatPerf.enabled()) {
        chatPerf.count('chat_watch_terminal', 1, {
          eventType: normalizedEventType || eventType,
          sessionId: key
        });
      }
    }
    return;
  };

  const baseEventId = threadChangeCursor(runtime);
  // Freeze the protocol branch for this watch request. A delayed ready frame
  // can update the connection capability after a legacy watch has started;
  // its handlers must never switch reducers mid-subscription.
  let useChangeStream = isChatThreadV2Session(key);
  startWatchdog();
  const watchPromise = chatWsClient.request({
      requestId,
      sessionId: key,
      message: () => {
        useChangeStream = isChatThreadV2Session(key) && isChatChangeStreamServerSupported();
        return {
          type: 'watch',
          request_id: requestId,
          session_id: key,
          payload: useChangeStream
            ? { change_stream: true, after_change_seq: getChatThreadState(key)?.lastSeq ?? 0 }
            : { after_event_id: baseEventId }
        };
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
        if (chatPerf.enabled()) {
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
  pingIntervalMs: 20000,
  readyTimeoutMs: 150,
  onConnecting: () => setChatChangeStreamServerSupported(null),
  onReady: (payload) => {
    const features = payload?.features;
    const supported = Boolean(
      features && typeof features === 'object' && !Array.isArray(features) &&
      (features as Record<string, unknown>).change_stream === true
    );
    setChatChangeStreamServerSupported(supported);
  }
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
  if (!Number.isSafeInteger(cursor) || cursor <= 0) {
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
