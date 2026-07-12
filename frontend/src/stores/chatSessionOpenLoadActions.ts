import { defineStore } from 'pinia';

import {
  archiveSession as archiveSessionApi,
  cancelMessageStream,
  compactSession as compactSessionApi,
  controlSessionSubagents as controlSessionSubagentsApi,
  createSession,
  deleteSession as deleteSessionApi,
  getSessionWithParams,
  getSessionGoal,
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
import type { ChatRuntimeProjection } from '@/realtime/chat/chatRuntimeTypes';
import {
  clearTrailingPendingAssistantMessages,
  clearSupersededPendingAssistantMessages,
  findPendingAssistantMessage,
  isPendingAssistantMessage,
  stopPendingAssistantMessage
} from './chatPendingMessage';
import {
  buildExistingHistoryIdSet,
  collectDedupedHistoryBackfillPage,
  normalizeHistoryBeforeId,
  prependHistoryBackfillPage,
  readHistoryBackfillPage
} from './chatHistoryBackfill';
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

import { dismissStaleInquiryPanels, ensureGreetingMessage, hydrateSessionCommandSessions, sortSessionsByActivity, syncDemoChatCache } from './chatDemoPanels';
import { hydrateMessage } from './chatMessageHydration';
import { DEFAULT_AGENT_KEY, applyMainSession, patchSessionRuntimeFields, persistActiveSession, persistAgentSession, persistDraftSession, syncGoalFromSessionRecord, syncGoalsFromSessionList } from './chatPersist';
import { HISTORY_PAGE_LIMIT, clearDraftSessionBootstrapMarkers, clearRuntimeInteractiveControllers, clearSessionWatcher, normalizeHistoryPageLimit, recoverRuntimeInteractiveControllers, resolveKnownSessionEventFloor, resolveMaterializedMessageEventId, resolveMessageWindowMax, resolveSessionDetailMessageLimit, setSessionLoading } from './chatRuntimeControls';
import { applyCanonicalSessionEventsSnapshot, applyHistoryMeta, applyLocalChatMessageRuntimeEvent, applyMessageWindow, applySessionRuntimeSnapshot, buildMessageIdentityDebugList, buildRuntimeDebugSnapshot, buildSessionHydratedMessageVersion, cacheSessionDetailSnapshot, cacheSessionMessages, clearCompletedAssistantStreamingState, countAssistantStreamingMessages, ensureRuntime, filterSessionsByAgent, findOldestHistoryId, getHistoryState, getSessionMessages, hasCanonicalSessionTranscript, hasKnownSessionInStore, isSessionDetailWarm, isSessionUnavailableStatus, loadSessionEventsSnapshot, markSessionDetailWarm, mergeForegroundHydratedMessagesWithLive, mergeRetainedActiveSessionIntoList, notifySessionSnapshot, purgeUnavailableSession, readSessionDetailSnapshot, readSessionEventsSnapshot, readSessionHydratedMessageVersion, readSessionListCacheEntry, refreshRuntimeStreamLifecycle, resolveCanonicalSessionTranscript, resolveChatHttpStatus, resolveSessionKey, resolveSessionListCacheKey, resolveSessionMessageArray, sessionDetailPrefetchInFlight, sessionListCacheInFlight, shouldApplySessionEventsSnapshotToProjection, shouldPreferCachedMessages, syncChatRuntimeProjectionFromSnapshot, touchSessionUpdatedAt, writeSessionHydratedMessageVersion, writeSessionListCache } from './chatRuntimeState';
import { normalizeSnapshotMessage } from './chatSnapshot';
import { buildMessage } from './chatStats';
import { normalizeStreamEventId, updateRuntimeLastEventId, updateRuntimeRemoteLastEventId } from './chatStreamIds';
import {
  ALL_SESSION_LIST_CACHE_KEY,
  LOAD_SESSIONS_BACKGROUND_REFRESH_MIN_AGE_MS,
  normalizeSessionListItems,
  resolveLoadSessionsCacheKey as resolveLoadSessionsCacheKeyBase
} from './chatSessionListLoadCache';
import { AppendLocalMessageOptions, CreateSessionOptions, LoadSessionDetailOptions, LoadSessionsOptions, OpenDraftSessionOptions } from './chatTypes';
import { abortResumeStream, startSessionWatcher } from './chatWatcher';
import { attachWorkflowEvents, getSessionWorkflowState, summarizeCompactionRoundEvents } from './chatWorkflowHydration';
import {
  shouldKeepActiveSessionWarmAfterHydration,
  shouldStartWatcherAfterSessionHydration
} from './chatActiveSessionRealtime';

const resolveLoadSessionsCacheKey = (agentId: string | null) =>
  resolveLoadSessionsCacheKeyBase(agentId, resolveSessionListCacheKey);

const readLoadSessionsCacheEntry = (agentId: string | null, maxAgeMs: number) =>
  agentId === null
    ? readSessionListCacheEntry(ALL_SESSION_LIST_CACHE_KEY, { maxAgeMs })
    : readSessionListCacheEntry(agentId, { maxAgeMs });

const writeLoadSessionsCache = (agentId: string | null, sessions: Record<string, unknown>[]) => {
  writeSessionListCache(agentId === null ? ALL_SESSION_LIST_CACHE_KEY : agentId, sessions);
};

const resolveSessionOpenDetailLimit = (): number => resolveSessionDetailMessageLimit(isDesktopModeEnabled());

const sessionDetailLoadInFlight = new Map<string, Promise<unknown>>();
const sessionDetailFetchControllers = new Map<string, AbortController>();

const resolveSessionDetailLoadInFlightKey = (
  sessionId: string,
  options: LoadSessionDetailOptions
): string =>
  [
    sessionId,
    options.preserveWatcher === true ? 'preserve' : 'replace',
    options.forceHydrateForeground === true ? 'force' : 'normal',
    options.startWatcherAfterHydration === false ? 'no-watch' : 'auto-watch'
  ].join('|');

const withSessionDetailLoadInFlight = <T>(
  sessionId: string,
  options: LoadSessionDetailOptions,
  loader: () => Promise<T>
): Promise<T> => {
  const key = resolveSessionDetailLoadInFlightKey(sessionId, options);
  const existing = sessionDetailLoadInFlight.get(key);
  if (existing) {
    return existing as Promise<T>;
  }
  const request = loader().finally(() => {
    if (sessionDetailLoadInFlight.get(key) === request) {
      sessionDetailLoadInFlight.delete(key);
    }
  });
  sessionDetailLoadInFlight.set(key, request);
  return request;
};

const isAbortLikeError = (error: unknown): boolean => {
  const record = error && typeof error === 'object'
    ? error as Record<string, unknown>
    : {};
  const name = String(record.name || '').trim();
  const message = String(record.message || error || '').trim().toLowerCase();
  return name === 'AbortError' ||
    name === 'CanceledError' ||
    message.includes('aborted') ||
    message.includes('canceled') ||
    message.includes('cancelled');
};

const abortDesktopSessionDetailLoadsExcept = (sessionId: string): void => {
  if (!isDesktopModeEnabled()) return;
  for (const [key, controller] of sessionDetailFetchControllers.entries()) {
    if (key === sessionId) continue;
    if (!controller.signal.aborted) {
      controller.abort();
    }
    sessionDetailFetchControllers.delete(key);
    for (const requestKey of Array.from(sessionDetailLoadInFlight.keys())) {
      if (requestKey.startsWith(`${key}|`)) {
        sessionDetailLoadInFlight.delete(requestKey);
      }
    }
  }
};

const createDesktopSessionDetailController = (sessionId: string): AbortController | null => {
  if (!isDesktopModeEnabled()) return null;
  abortDesktopSessionDetailLoadsExcept(sessionId);
  const existing = sessionDetailFetchControllers.get(sessionId);
  if (existing && !existing.signal.aborted) {
    return existing;
  }
  const controller = new AbortController();
  sessionDetailFetchControllers.set(sessionId, controller);
  return controller;
};

const clearDesktopSessionDetailController = (
  sessionId: string,
  controller: AbortController | null
): void => {
  if (!controller) return;
  if (sessionDetailFetchControllers.get(sessionId) === controller) {
    sessionDetailFetchControllers.delete(sessionId);
  }
};

const isStaleDesktopSessionDetailLoad = (
  store: { activeSessionId?: unknown } | null | undefined,
  sessionId: string,
  preserveWatcher: boolean
): boolean =>
  isDesktopModeEnabled() &&
  !preserveWatcher &&
  resolveSessionKey(store?.activeSessionId) !== sessionId;

export const chatSessionOpenLoadActions = {
    appendLocalMessage(role: string, content: string, options: AppendLocalMessageOptions = {}) {
      const normalizedRole = role === 'assistant' ? 'assistant' : 'user';
      const text = String(content || '').trim();
      if (!text) return null;
      const message = buildMessage(normalizedRole, text, options.createdAt);
      const createdAt = String(message.created_at || new Date().toISOString());
      const localId = `local-ui:${Date.parse(createdAt) || Date.now()}:${Math.random().toString(16).slice(2)}`;
      const localTurnId = String(options.localTurnId || '').trim() || `local-ui-turn:${localId}`;
      const localModelTurnId = normalizedRole === 'assistant'
        ? String(options.localModelTurnId || '').trim() || `local-ui-model:${localId}`
        : '';
      Object.assign(message, {
        message_id: localId,
        client_message_id: localId,
        user_turn_id: localTurnId,
        ...(localModelTurnId ? { model_turn_id: localModelTurnId } : {})
      });
      if (options.manualGoalMarker === true && normalizedRole === 'assistant') {
        message.manual_goal_marker = true;
      }
      this.messages.push(message);
      const targetSessionId = String(options.sessionId ?? this.activeSessionId ?? '').trim();
      if (targetSessionId) {
        applyLocalChatMessageRuntimeEvent(this, {
          sessionId: targetSessionId,
          role: normalizedRole,
          content: text,
          messageId: localId,
          createdAt,
          userTurnId: localTurnId,
          modelTurnId: localModelTurnId,
          display: {
            client_message_id: localId,
            ...(options.manualGoalMarker === true ? { manual_goal_marker: true, manualGoalMarker: true } : {})
          }
        });
        cacheSessionMessages(targetSessionId, this.messages);
        touchSessionUpdatedAt(this, targetSessionId, message.created_at || Date.now());
        notifySessionSnapshot(this, targetSessionId, this.messages, options.immediate !== false);
      } else {
        this.scheduleSnapshot(options.immediate !== false);
      }
      return message;
    },
    async loadSessions(options: LoadSessionsOptions = {}) {
      const params: { agent_id?: string } = {};
      let requestedAgentId: string | null = null;
      const traceId = String((options as Record<string, unknown> | null)?.traceId || '').trim();
      const traceSource = String((options as Record<string, unknown> | null)?.traceSource || '').trim();
      const force = (options as Record<string, unknown> | null)?.force === true;
      const preferCache = (options as Record<string, unknown> | null)?.preferCache === true;
      const backgroundRefresh = (options as Record<string, unknown> | null)?.backgroundRefresh === true;
      const requestedMaxCacheAgeMs = Number((options as Record<string, unknown> | null)?.maxCacheAgeMs);
      const maxCacheAgeMs = Number.isFinite(requestedMaxCacheAgeMs)
        ? Math.max(0, requestedMaxCacheAgeMs)
        : 15000;

      if (Object.prototype.hasOwnProperty.call(options, 'agent_id')) {
        requestedAgentId = String(options.agent_id ?? '');
        params.agent_id = requestedAgentId;
      }
      const now = Date.now();
      const activeSessionKey = resolveSessionKey(this.activeSessionId);
      const activeRuntime = ensureRuntime(activeSessionKey);
      const activeRuntimeInteractive = Boolean(
        activeSessionKey &&
          (
            activeRuntime?.sendController ||
            activeRuntime?.resumeController
          )
      );
      const activeRuntimeHot = Boolean(
        activeSessionKey &&
          (
            this.loadingBySession?.[activeSessionKey] ||
            activeRuntime?.sendController ||
            activeRuntime?.resumeController
          )
      );
      const recentListRefreshAgeMs = now - Number(this.sessionsLoadedAt || 0);
      if (
        !force &&
        traceSource === 'realtime-pulse' &&
        requestedAgentId === null &&
        activeRuntimeInteractive
      ) {
        chatDebugLog('messenger.conversation', 'load-sessions-skip-interactive-stream', {
          traceId,
          traceSource,
          requestedAgentId,
          activeSessionId: activeSessionKey,
          previousSessionCount: Array.isArray(this.sessions) ? this.sessions.length : 0,
          ageMs: recentListRefreshAgeMs
        });
        return this.sessions;
      }
      if (
        !force &&
        traceSource === 'realtime-pulse' &&
        requestedAgentId === null &&
        !activeRuntimeHot &&
        Number(this.sessionsLoadedAt || 0) > 0 &&
        recentListRefreshAgeMs < 5000
      ) {
        chatDebugLog('messenger.conversation', 'load-sessions-skip', {
          traceId,
          traceSource,
          requestedAgentId,
          activeSessionId: activeSessionKey,
          previousSessionCount: Array.isArray(this.sessions) ? this.sessions.length : 0,
          ageMs: recentListRefreshAgeMs
        });
        return this.sessions;
      }
      const cacheKey = resolveLoadSessionsCacheKey(requestedAgentId);
      const applyLoadedSessions = (
        items: unknown,
        source: string,
        options: { writeCache?: boolean; loadedAt?: number } = {}
      ) => {
        const nextSessions = mergeSessionsByIdPreservingRuntimeFields(
          this.sessions,
          normalizeSessionListItems(items),
          patchSessionRuntimeFields,
          sortSessionsByActivity
        );
        this.sessions = mergeRetainedActiveSessionIntoList(this, nextSessions);
        this.sessionsLoadedAt = Number.isFinite(Number(options.loadedAt))
          ? Number(options.loadedAt)
          : Date.now();
        syncGoalsFromSessionList(this, this.sessions);
        if (options.writeCache !== false) {
          writeLoadSessionsCache(requestedAgentId, this.sessions);
        }
        syncDemoChatCache({ sessions: this.sessions });
        chatDebugLog('messenger.conversation', source, {
          traceId,
          traceSource,
          requestedAgentId,
          cacheKey,
          activeSessionId: resolveSessionKey(this.activeSessionId),
          ageMs: Date.now() - Number(this.sessionsLoadedAt || 0),
          nextSessionCount: Array.isArray(this.sessions) ? this.sessions.length : 0
        });
        return this.sessions;
      };
      const refreshSessions = async (source: string) => {
        const { data } = await listSessions(Object.keys(params).length ? params : undefined);
        return applyLoadedSessions(data?.data?.items || [], source);
      };
      const scheduleBackgroundRefresh = (fallbackSessions: Record<string, unknown>[], ageMs: number) => {
        let backgroundRequest = sessionListCacheInFlight.get(cacheKey);
        if (!backgroundRequest) {
          chatDebugLog('messenger.conversation', 'load-sessions-background-start', {
            traceId,
            traceSource,
            requestedAgentId,
            cacheKey,
            activeSessionId: activeSessionKey,
            previousSessionCount: Array.isArray(this.sessions) ? this.sessions.length : 0,
            ageMs
          });
          backgroundRequest = refreshSessions('load-sessions-background-finish')
            .catch((error) => {
              chatDebugLog('messenger.conversation', 'load-sessions-background-error', {
                traceId,
                traceSource,
                requestedAgentId,
                cacheKey,
                message: error instanceof Error ? error.message : String(error || '')
              });
              return fallbackSessions;
            })
            .finally(() => {
              sessionListCacheInFlight.delete(cacheKey);
            });
          sessionListCacheInFlight.set(cacheKey, backgroundRequest);
        }
        return backgroundRequest;
      };
      if (!force && preferCache) {
        const cachedEntry = readLoadSessionsCacheEntry(requestedAgentId, maxCacheAgeMs);
        if (cachedEntry) {
          const cachedAgeMs = Date.now() - Number(cachedEntry.cachedAt || 0);
          const cached = applyLoadedSessions(cachedEntry.sessions, 'load-sessions-cache-hit', {
            writeCache: false,
            loadedAt: cachedEntry.cachedAt
          });
          if (
            !backgroundRefresh ||
            activeRuntimeHot ||
            cachedAgeMs < LOAD_SESSIONS_BACKGROUND_REFRESH_MIN_AGE_MS
          ) {
            return cached;
          }
          scheduleBackgroundRefresh(cached, cachedAgeMs);
          return cached;
        }
        const memorySessions = requestedAgentId === null
          ? normalizeSessionListItems(this.sessions)
          : normalizeSessionListItems(filterSessionsByAgent(requestedAgentId, this.sessions));
        if (memorySessions.length) {
          const loadedAt = Number(this.sessionsLoadedAt || 0);
          const memoryAgeMs = loadedAt > 0 ? Date.now() - loadedAt : Number.POSITIVE_INFINITY;
          const cached = applyLoadedSessions(memorySessions, 'load-sessions-memory-hit', {
            writeCache: false,
            loadedAt
          });
          if (
            !backgroundRefresh ||
            activeRuntimeHot ||
            memoryAgeMs < LOAD_SESSIONS_BACKGROUND_REFRESH_MIN_AGE_MS
          ) {
            return cached;
          }
          scheduleBackgroundRefresh(cached, memoryAgeMs);
          return cached;
        }
      }
      const inFlight = !force ? sessionListCacheInFlight.get(cacheKey) : null;
      if (inFlight) {
        chatDebugLog('messenger.conversation', 'load-sessions-inflight-hit', {
          traceId,
          traceSource,
          requestedAgentId,
          cacheKey,
          activeSessionId: activeSessionKey,
          previousSessionCount: Array.isArray(this.sessions) ? this.sessions.length : 0
        });
        return Promise.resolve(inFlight).then((sessions) =>
          applyLoadedSessions(
            Array.isArray(sessions) ? sessions : this.sessions,
            'load-sessions-inflight-finish'
          )
        );
      }
      chatDebugLog('messenger.conversation', 'load-sessions-start', {
        traceId,
        traceSource,
        requestedAgentId,
        cacheKey,
        activeSessionId: activeSessionKey,
        previousSessionCount: Array.isArray(this.sessions) ? this.sessions.length : 0
      });
      const request = refreshSessions('load-sessions-finish')
        .finally(() => {
          sessionListCacheInFlight.delete(cacheKey);
        });
      sessionListCacheInFlight.set(cacheKey, request);
      return request;
    },
    openDraftSession(options: OpenDraftSessionOptions = {}) {
      const currentSessionId = this.activeSessionId;
      cacheSessionMessages(currentSessionId, this.messages);
      abortResumeStream(currentSessionId);
      clearSessionWatcher();
      const runtime = ensureRuntime(currentSessionId);
      if (runtime) {
        runtime.stopRequested = false;
      }
      // Keep in-flight send stream alive so switching agent/thread won't cancel background runs.
      if (!runtime?.sendController) {
        setSessionLoading(this, currentSessionId, false);
      }
      this.activeSessionId = null;
      this.draftAgentId = String(options.agent_id || '').trim();
      this.draftToolOverrides = null;
      this.messages = ensureGreetingMessage([], { greeting: this.greetingOverride });
      persistDraftSession();
    },
    setDraftToolOverrides(overrides) {
      if (!Array.isArray(overrides) || overrides.length === 0) {
        this.draftToolOverrides = null;
        return;
      }
      this.draftToolOverrides = [...overrides];
    },
    async createSession(payload: Record<string, unknown> = {}, options: CreateSessionOptions = {}) {
      abortResumeStream(this.activeSessionId);
      clearSessionWatcher();
      const { data } = await createSession(payload);
      const session = patchSessionRuntimeFields(data.data);
      this.sessions.unshift(session);
      syncGoalFromSessionRecord(this, session);
      if (session?.is_main === true) {
        this.sessions = applyMainSession(this.sessions, session.agent_id, session.id);
      }
      writeSessionListCache(session.agent_id, filterSessionsByAgent(session.agent_id, this.sessions));
      this.activeSessionId = session.id;
      this.draftAgentId = String(session.agent_id || '').trim();
      const baseMessages = options.preserveCurrentMessages === true ? this.messages : [];
      this.messages = ensureGreetingMessage(baseMessages, {
        createdAt: session.created_at,
        greeting: this.greetingOverride
      });
      clearDraftSessionBootstrapMarkers(this.messages);
      cacheSessionMessages(session.id, this.messages);
      touchSessionUpdatedAt(this, session.id, session.updated_at || session.created_at);
      getSessionWorkflowState(session.id, { reset: true });
      getHistoryState(session.id, { reset: true });
      persistActiveSession(session.id, session.agent_id);
      syncDemoChatCache({
        sessions: this.sessions,
        sessionId: this.activeSessionId,
        messages: this.messages
      });
      if (session?.is_main !== true) {
        // Keep creation flow responsive; main-session sync can finish in background.
        void this.setMainSession(session.id).catch(() => {
          // Keep local session state when explicit main-session sync fails.
        });
      }
      startSessionWatcher(this, session.id);
      return session;
    },
    async setMainSession(sessionId) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return null;
      const targetSession = this.sessions.find((item) => item.id === targetId) || null;
      const agentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      const apiAgentId = agentId || DEFAULT_AGENT_KEY;
      await setDefaultSession(apiAgentId, { session_id: targetId });
      this.sessions = applyMainSession(this.sessions, agentId, targetId);
      writeSessionListCache(agentId, filterSessionsByAgent(agentId, this.sessions));
      persistAgentSession(agentId, targetId);
      return targetId;
    },

    async loadSessionDetail(sessionId, options: LoadSessionDetailOptions = {}) {
      const targetSessionId = resolveSessionKey(sessionId);
      if (!targetSessionId) return null;
      return withSessionDetailLoadInFlight(targetSessionId, options, async () => {
        const perfEnabled = chatPerf.enabled();
        const perfStart = perfEnabled ? performance.now() : 0;
        let perfFetchStart = 0;
        let perfFetchMs: number | null = null;
        let perfHydrateStart = 0;
        let perfHydrateMs: number | null = null;
        let perfForegroundSyncStart = 0;
        let perfForegroundSyncMs: number | null = null;
        let perfReusedHydratedMessages = false;
        let perfRemoteRunning = false;
        let perfTranscriptCount = 0;
        let perfEventCount = 0;
        let perfRoundCount = 0;
        const previousSessionId = this.activeSessionId;
        const previousSessionKey = resolveSessionKey(previousSessionId);
        const previousForegroundMessages = Array.isArray(this.messages) ? this.messages : [];
        const runtimeForPreserveGuard = ensureRuntime(targetSessionId);
        recoverRuntimeInteractiveControllers(this, targetSessionId, runtimeForPreserveGuard);
        const lifecycleForPreserveGuard = refreshRuntimeStreamLifecycle(runtimeForPreserveGuard);
        const preserveWatcher =
          previousSessionKey === targetSessionId &&
          (
            options.preserveWatcher === true ||
            shouldForcePreserveWatcherForActiveSession({
              isSameActiveSession: true,
              lifecycle: lifecycleForPreserveGuard,
              hasSendController: Boolean(runtimeForPreserveGuard?.sendController),
              hasResumeController: Boolean(runtimeForPreserveGuard?.resumeController)
            })
          );
        if (previousSessionId && previousSessionId !== targetSessionId) {
          cacheSessionMessages(previousSessionId, this.messages);
        }
        if (!preserveWatcher) {
          abortResumeStream(previousSessionId);
          clearSessionWatcher();
          this.activeSessionId = targetSessionId;
          abortDesktopSessionDetailLoadsExcept(targetSessionId);
        }
        const detailAbortController = createDesktopSessionDetailController(targetSessionId);
        getHistoryState(targetSessionId, { reset: true });
        const knownSessionRecord =
          this.sessions.find((item) => resolveSessionKey(item?.id) === targetSessionId) || null;
        const liveSessionMessages = resolveSessionMessageArray(
          {
            activeSessionId: previousSessionKey,
            messages: previousForegroundMessages
          },
          targetSessionId,
          previousSessionKey === targetSessionId ? previousForegroundMessages : null
        );
        const cachedSessionMessages = liveSessionMessages;
        const snapshot = previousSessionKey && previousSessionKey !== targetSessionId
          ? null
          : this.getSnapshotForSession(targetSessionId);
        if (cachedSessionMessages?.length) {
          this.messages = ensureGreetingMessage(cachedSessionMessages, {
            greeting: this.greetingOverride
          });
        } else if (snapshot?.messages?.length) {
          const cachedMessages = snapshot.messages
            .map((item) => normalizeSnapshotMessage(item))
            .filter(Boolean);
          this.messages = ensureGreetingMessage(cachedMessages, {
            greeting: this.greetingOverride
          });
        } else if (!preserveWatcher) {
          // Prevent a session switch from momentarily reusing the previous thread's foreground messages.
          this.messages = ensureGreetingMessage([], {
            createdAt: knownSessionRecord?.created_at,
            greeting: this.greetingOverride
          });
        }
        if (cachedSessionMessages?.length || snapshot?.messages?.length) {
          cacheSessionMessages(targetSessionId, this.messages);
        }
        if (!hasKnownSessionInStore(this, targetSessionId)) {
          purgeUnavailableSession(this, targetSessionId);
          return null;
        }
        const pendingPrefetch = sessionDetailPrefetchInFlight.get(targetSessionId);
        let prefetchedSessionDetail = null;
        if (pendingPrefetch) {
          try {
            prefetchedSessionDetail = await pendingPrefetch;
          } catch (error) {
            prefetchedSessionDetail = null;
          }
        }
        const latestCachedSessionMessages = resolveSessionMessageArray(
            {
              activeSessionId: previousSessionKey,
              messages: previousForegroundMessages
            },
            targetSessionId,
            previousSessionKey === targetSessionId ? previousForegroundMessages : null
        );
        let sessionRes = null;
        let eventsPayload = null;
        let sessionDetail = prefetchedSessionDetail;
        const detailLimit = resolveSessionOpenDetailLimit();
        const knownEventFloor = resolveKnownSessionEventFloor(
          targetSessionId,
          latestCachedSessionMessages || cachedSessionMessages
        );
        if (!sessionDetail && isSessionDetailWarm(targetSessionId)) {
          sessionDetail = readSessionDetailSnapshot(targetSessionId);
        }
        if (sessionDetail && isSessionDetailWarm(targetSessionId)) {
          eventsPayload = readSessionEventsSnapshot(targetSessionId, {
            // Avoid trusting stale running cache when switching into a historical thread.
            // If cache says running, fall back to fresh events snapshot request below.
            allowRunning: false,
            minLastEventId: knownEventFloor,
            limit: detailLimit
          });
        }
        try {
          if (!sessionDetail || !eventsPayload) {
            perfFetchStart = perfEnabled ? performance.now() : 0;
            [sessionRes, eventsPayload] = await Promise.all([
              getSessionWithParams(
                targetSessionId,
                { limit: detailLimit, summary: true },
                detailAbortController ? { signal: detailAbortController.signal } : {}
              ),
              loadSessionEventsSnapshot(targetSessionId, {
                limit: detailLimit,
                minLastEventId: knownEventFloor,
                shouldCache: () => !isStaleDesktopSessionDetailLoad(this, targetSessionId, preserveWatcher),
                ...(detailAbortController ? { signal: detailAbortController.signal } : {})
              }).catch((error) => {
                if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
                  throw error;
                }
                if (isAbortLikeError(error)) {
                  throw error;
                }
                return null;
              })
            ]);
            if (detailAbortController?.signal.aborted || isStaleDesktopSessionDetailLoad(this, targetSessionId, preserveWatcher)) {
              return null;
            }
            if (perfEnabled) {
              perfFetchMs = performance.now() - perfFetchStart;
            }
            sessionDetail = sessionRes?.data?.data || null;
            cacheSessionDetailSnapshot(targetSessionId, sessionDetail);
          }
        } catch (error) {
          if (isAbortLikeError(error)) {
            return null;
          }
          if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
            purgeUnavailableSession(this, targetSessionId);
            return null;
          }
          throw error;
        } finally {
          clearDesktopSessionDetailController(targetSessionId, detailAbortController);
        }
        if (isStaleDesktopSessionDetailLoad(this, targetSessionId, preserveWatcher)) {
          return null;
        }
        const data = sessionRes?.data;
        const detailTranscriptCount = Array.isArray(sessionDetail?.transcript)
          ? sessionDetail.transcript.length
          : 0;
        const detailEventCount = Array.isArray(eventsPayload?.events)
          ? eventsPayload.events.length
          : 0;
        perfTranscriptCount = detailTranscriptCount;
        perfEventCount = detailEventCount;
        syncGoalFromSessionRecord(this, sessionDetail);
        const hydratedVersion = buildSessionHydratedMessageVersion(sessionDetail, eventsPayload);
        hydrateSessionCommandSessions(
          targetSessionId,
          eventsPayload?.command_sessions ?? eventsPayload?.commandSessions
        );
        const runtime = ensureRuntime(targetSessionId);
        applySessionRuntimeSnapshot(runtime, eventsPayload?.runtime);
        const remoteRunning = eventsPayload?.running === true;
        const remoteLastEventId = normalizeStreamEventId(
          eventsPayload?.last_event_id ?? eventsPayload?.lastEventId
        );
        updateRuntimeRemoteLastEventId(runtime, remoteLastEventId);
        if (shouldApplySessionEventsSnapshotToProjection(eventsPayload, runtime)) {
          applyCanonicalSessionEventsSnapshot(this, targetSessionId, eventsPayload, {
            phase: 'detail'
          });
        } else {
          chatDebugLog('chat.store.detail', 'events-snapshot-skip-idle-transcript', {
            sessionId: targetSessionId,
            remoteRunning,
            remoteLastEventId,
            eventCount: detailEventCount,
            transcriptCount: detailTranscriptCount,
            runtime: buildRuntimeDebugSnapshot(runtime)
          });
        }
        recoverRuntimeInteractiveControllers(this, targetSessionId, runtime, {
          remoteRunning: eventsPayload?.running,
          remoteLastEventId,
          localLastEventId: resolveMaterializedMessageEventId(
            getSessionMessages(targetSessionId) || (resolveSessionKey(this.activeSessionId) === targetSessionId
              ? this.messages
              : [])
          )
        });
        if (eventsPayload?.running === false) {
          clearRuntimeInteractiveControllers(runtime, { abort: false });
        }
        const sessionCreatedAt = sessionDetail?.created_at;
        if (sessionDetail?.id) {
          const index = this.sessions.findIndex((item) => item.id === sessionDetail.id);
          if (index >= 0) {
            this.sessions[index] = patchSessionRuntimeFields({ ...this.sessions[index], ...sessionDetail });
          } else {
            this.sessions.unshift(patchSessionRuntimeFields(sessionDetail));
          }
        }
        const resolvedAgentId =
          sessionDetail?.agent_id ??
          this.sessions.find((item) => item.id === targetSessionId)?.agent_id ??
          '';
        const resolvedAgentIdText = String(resolvedAgentId || '').trim();
        writeSessionListCache(
          resolvedAgentIdText,
          filterSessionsByAgent(resolvedAgentIdText, this.sessions)
        );
        const rounds = eventsPayload?.rounds || [];
        perfRemoteRunning = remoteRunning;
        perfRoundCount = Array.isArray(rounds) ? rounds.length : 0;
        chatDebugLog('chat.store.detail', 'payload-loaded', {
          sessionId: targetSessionId,
          desktopMode: isDesktopModeEnabled(),
          transcriptCount: detailTranscriptCount,
          eventCount: detailEventCount,
          roundCount: Array.isArray(rounds) ? rounds.length : 0,
          limit: resolveSessionOpenDetailLimit()
        });
        const compactionHydrationRounds = Array.isArray(rounds)
          ? rounds
              .map((round) => {
                const roundNumber = Number(round?.user_round ?? round?.round);
                const summary = summarizeCompactionRoundEvents(round?.events);
                if (!summary) return null;
                return { round: roundNumber, ...summary };
              })
              .filter(Boolean)
          : [];
        const finalCachedMessages = resolveSessionMessageArray(
            this,
            targetSessionId,
            resolveSessionKey(this.activeSessionId) === targetSessionId ? this.messages : null
        );
        const previousHydratedVersion = readSessionHydratedMessageVersion(targetSessionId);
        const hasCanonicalTranscript = hasCanonicalSessionTranscript(sessionDetail);
        const canReuseHydratedMessages =
          !hasCanonicalTranscript &&
          !remoteRunning &&
          Array.isArray(finalCachedMessages) &&
          finalCachedMessages.length > 0 &&
          previousHydratedVersion === hydratedVersion;
        perfReusedHydratedMessages = canReuseHydratedMessages;
        let messages = finalCachedMessages;
        perfHydrateStart = perfEnabled ? performance.now() : 0;
        if (canReuseHydratedMessages) {
          getSessionWorkflowState(targetSessionId, { reset: true });
          messages = finalCachedMessages;
        } else {
          const workflowState = getSessionWorkflowState(targetSessionId, { reset: true });
          const rawMessages = attachWorkflowEvents(
            resolveCanonicalSessionTranscript(sessionDetail),
            rounds
          );
          messages = rawMessages.map((message) =>
            hydrateMessage(message, workflowState)
          );
          if (!hasCanonicalTranscript) {
            messages = mergeCompactionMarkersIntoMessages(messages, finalCachedMessages);
          }
        }
        if (perfEnabled) {
          perfHydrateMs = performance.now() - perfHydrateStart;
        }
        if (compactionHydrationRounds.length > 0) {
          chatDebugLog('chat.compaction.hydrate', 'load-session-detail', {
            sessionId: targetSessionId,
            remoteRunning,
            roundCount: rounds.length,
            cachedMessageCount: Array.isArray(finalCachedMessages) ? finalCachedMessages.length : 0,
            hydratedMessageCount: Array.isArray(messages) ? messages.length : 0,
            compactionRounds: compactionHydrationRounds
          });
        }
        if (!remoteRunning) {
          clearCompletedAssistantStreamingState(finalCachedMessages);
          clearCompletedAssistantStreamingState(messages);
        }
        if (!hasCanonicalTranscript && remoteRunning && shouldPreferCachedMessages(finalCachedMessages, messages)) {
          messages = finalCachedMessages;
        }
        dismissStaleInquiryPanels(messages);
        let nextMessages = ensureGreetingMessage(messages, {
          createdAt: sessionCreatedAt,
          greeting: this.greetingOverride
        });
        if (!remoteRunning) {
          clearCompletedAssistantStreamingState(nextMessages);
        }
        clearSupersededPendingAssistantMessages(nextMessages);
        applyHistoryMeta(targetSessionId, sessionDetail, nextMessages);
        const activeSessionKey = resolveSessionKey(this.activeSessionId);
        const shouldKeepStableForegroundMessages =
          !hasCanonicalTranscript &&
          preserveWatcher &&
          !remoteRunning &&
          previousHydratedVersion === hydratedVersion &&
          Array.isArray(finalCachedMessages) &&
          finalCachedMessages.length > 0;
        const hydrateForegroundMessages = shouldApplyForegroundDetailHydration({
          preserveWatcher,
          forceHydration: options.forceHydrateForeground === true,
          lifecycle: refreshRuntimeStreamLifecycle(runtime),
          hasWatchController: Boolean(runtime?.watchController),
          hasSendController: Boolean(runtime?.sendController),
          hasResumeController: Boolean(runtime?.resumeController)
        });
        const hasPendingAssistantAfterHydrationPreview = Boolean(findPendingAssistantMessage(nextMessages));
        const liveForegroundMessages =
          preserveWatcher || activeSessionKey === targetSessionId
            ? resolveSessionMessageArray(
                this,
                targetSessionId,
                activeSessionKey === targetSessionId ? this.messages : null
              )
            : null;
        const hasPendingAssistantInForegroundLive = Boolean(
          findPendingAssistantMessage(liveForegroundMessages)
        );
        const keepForegroundRunningGap = shouldKeepForegroundLiveMessagesDuringRunningGap({
          preserveWatcher,
          lifecycle: refreshRuntimeStreamLifecycle(runtime),
          hasSendController: Boolean(runtime?.sendController),
          hasResumeController: Boolean(runtime?.resumeController),
          remoteRunning,
          liveHasPendingAssistant: hasPendingAssistantInForegroundLive,
          hydratedHasPendingAssistant: hasPendingAssistantAfterHydrationPreview
        });
        chatDebugLog('chat.store.detail', 'foreground-sync-decision', {
          sessionId: targetSessionId,
          preserveWatcher,
          hydrateForegroundMessages,
          remoteRunning,
          activeSessionKey,
          keepForegroundRunningGap,
          hasPendingAssistantInForegroundLive,
          hasPendingAssistantAfterHydration: hasPendingAssistantAfterHydrationPreview,
          cachedMessageCount: Array.isArray(finalCachedMessages) ? finalCachedMessages.length : 0,
          nextMessageCount: Array.isArray(nextMessages) ? nextMessages.length : 0,
          compactionRoundCount: compactionHydrationRounds.length,
          runtime: buildRuntimeDebugSnapshot(runtime)
        });
        if (shouldKeepStableForegroundMessages) {
          const watchedMessages = resolveSessionMessageArray(
            this,
            targetSessionId,
            activeSessionKey === targetSessionId ? this.messages : null
          );
          if (Array.isArray(watchedMessages) && watchedMessages.length > 0) {
            nextMessages = watchedMessages;
          }
        } else if (keepForegroundRunningGap) {
          const watchedMessages = resolveSessionMessageArray(
            this,
            targetSessionId,
            activeSessionKey === targetSessionId ? this.messages : null
          );
          if (Array.isArray(watchedMessages) && watchedMessages.length > 0) {
            chatDebugLog('chat.store.detail', 'foreground-sync-preserve-running-gap', {
              sessionId: targetSessionId,
              watchedMessageCount: watchedMessages.length,
              hydratedMessageCount: Array.isArray(nextMessages) ? nextMessages.length : 0,
              compactionRoundCount: compactionHydrationRounds.length
            });
            nextMessages = watchedMessages;
          }
        } else if (shouldKeepForegroundLiveMessages({
          preserveWatcher,
          hydrateForegroundMessages,
          remoteRunning
        })) {
          const watchedMessages = resolveSessionMessageArray(
            this,
            targetSessionId,
            activeSessionKey === targetSessionId ? this.messages : null
          );
          if (Array.isArray(watchedMessages)) {
            chatDebugLog('chat.store.detail', 'foreground-sync-keep-live', {
              sessionId: targetSessionId,
              watchedMessageCount: watchedMessages.length,
              hydratedMessageCount: Array.isArray(nextMessages) ? nextMessages.length : 0,
              compactionRoundCount: compactionHydrationRounds.length
            });
            nextMessages = watchedMessages;
          }
        } else if (preserveWatcher) {
          const watchedMessages = resolveSessionMessageArray(
            this,
            targetSessionId,
            activeSessionKey === targetSessionId ? this.messages : null
          );
          const foregroundMerge = mergeForegroundHydratedMessagesWithLive(
            watchedMessages,
            nextMessages
          );
          const mergedWithCompactionMarkers = mergeCompactionMarkersIntoMessages(
            foregroundMerge.messages,
            watchedMessages
          );
          chatDebugLog('chat.store.detail', 'foreground-sync-replace-live', {
            sessionId: targetSessionId,
            watchedMessageCount: Array.isArray(watchedMessages) ? watchedMessages.length : 0,
            hydratedMessageCount: Array.isArray(nextMessages) ? nextMessages.length : 0,
            compactionRoundCount: compactionHydrationRounds.length,
            merge: {
              ...foregroundMerge.debug,
              markerMessageCountBefore:
                Array.isArray(foregroundMerge.messages)
                  ? foregroundMerge.messages.filter((message) =>
                      isCompactionMarkerAssistantMessage(message)
                    ).length
                  : 0,
              markerMessageCountAfter:
                Array.isArray(mergedWithCompactionMarkers)
                  ? mergedWithCompactionMarkers.filter((message) =>
                      isCompactionMarkerAssistantMessage(message)
                    ).length
                  : 0
            }
          });
          nextMessages = replaceMessageArrayKeepingReference(
            watchedMessages,
            mergedWithCompactionMarkers
          );
        }
        if (!remoteRunning) {
          const runningCountBeforeClear = countAssistantStreamingMessages(nextMessages);
          clearCompletedAssistantStreamingState(nextMessages);
          const runningCountAfterClear = countAssistantStreamingMessages(nextMessages);
          if (runningCountBeforeClear !== runningCountAfterClear) {
            chatDebugLog('chat.store.detail', 'idle-stream-state-cleared', {
              sessionId: targetSessionId,
              preserveWatcher,
              hydrateForegroundMessages,
              runningCountBeforeClear,
              runningCountAfterClear,
              messageCount: Array.isArray(nextMessages) ? nextMessages.length : 0
            });
          }
        }
        const hasPendingAssistantAfterHydration = Boolean(findPendingAssistantMessage(nextMessages));
        cacheSessionMessages(targetSessionId, nextMessages);
        updateRuntimeLastEventId(
          runtime,
          Math.max(resolveMaterializedMessageEventId(nextMessages), remoteLastEventId || 0)
        );
        const shouldKeepInteractiveRuntime = shouldKeepForegroundInteractiveRuntime({
          remoteRunning,
          hasSendController: Boolean(runtime?.sendController),
          hasResumeController: Boolean(runtime?.resumeController)
        });
        if (!hasPendingAssistantAfterHydration && !shouldKeepInteractiveRuntime) {
          clearRuntimeInteractiveControllers(runtime, { abort: false });
          // Keep historical threads idle on entry; avoid reviving stale running status.
          setSessionLoading(this, targetSessionId, false);
        }
        writeSessionHydratedMessageVersion(targetSessionId, hydratedVersion);
        markSessionDetailWarm(targetSessionId);
        // Ignore stale async response: keep current foreground conversation state untouched.
        if (activeSessionKey !== targetSessionId) {
          return sessionDetail;
        }
        perfForegroundSyncStart = perfEnabled ? performance.now() : 0;
        this.draftAgentId = resolvedAgentIdText;
        persistActiveSession(targetSessionId, resolvedAgentIdText);
        this.draftToolOverrides = null;
        if (this.messages !== nextMessages) {
          this.messages = nextMessages;
        }
        syncChatRuntimeProjectionFromSnapshot(this, targetSessionId, this.messages, {
          immediate: true,
          loading: remoteRunning,
          running: remoteRunning,
          authoritative: !remoteRunning
        });
        applyMessageWindow(this, targetSessionId, this.messages);
        syncDemoChatCache({ sessionId: targetSessionId, messages: this.messages });
        if (perfEnabled) {
          perfForegroundSyncMs = performance.now() - perfForegroundSyncStart;
        }
        if (hydrateForegroundMessages) {
          const pendingMessage = findPendingAssistantMessage(this.messages);
          if (pendingMessage && remoteRunning) {
            const resumeAfterEventId =
              normalizeStreamEventId(pendingMessage.stream_event_id) ?? remoteLastEventId;
            if (
              resumeAfterEventId !== null &&
              resumeAfterEventId > 0 &&
              normalizeStreamEventId(pendingMessage.stream_event_id) === null
            ) {
              pendingMessage.stream_event_id = resumeAfterEventId;
            }
            this.resumeStream(
              targetSessionId,
              pendingMessage,
              resumeAfterEventId !== null && resumeAfterEventId > 0
                ? { afterEventId: resumeAfterEventId }
                : {}
            );
          } else {
            if (!remoteRunning) {
              clearCompletedAssistantStreamingState(this.messages);
            }
            setSessionLoading(this, targetSessionId, false);
          }
        }
        chatDebugLog('chat.store.detail', 'hydration-message-identity', {
          sessionId: targetSessionId,
          hasCanonicalTranscript,
          remoteRunning,
          ...(isChatDebugVerboseEnabled()
            ? {
                transcriptSummary: buildMessageIdentityDebugList(
                  resolveCanonicalSessionTranscript(sessionDetail)
                ),
                hydratedSummary: buildMessageIdentityDebugList(nextMessages),
                foregroundSummary: buildMessageIdentityDebugList(this.messages)
              }
            : {
                transcriptCount: resolveCanonicalSessionTranscript(sessionDetail).length,
                hydratedCount: Array.isArray(nextMessages) ? nextMessages.length : 0,
                foregroundCount: Array.isArray(this.messages) ? this.messages.length : 0
              })
        });
        this.scheduleSnapshot(true);
        const allowStartWatcherAfterHydration = options.startWatcherAfterHydration !== false;
        const shouldKeepActiveSessionWarm = shouldKeepActiveSessionWarmAfterHydration({
          isActiveSession: activeSessionKey === targetSessionId,
          desktopMode: isDesktopModeEnabled(),
          remoteRunning,
          runtimeStatus: runtime?.threadStatus,
          hasPendingAssistant: hasPendingAssistantAfterHydration
        });
        const shouldStartWatcher =
          allowStartWatcherAfterHydration &&
          (
            !preserveWatcher ||
            activeSessionKey === targetSessionId
          ) &&
          shouldStartWatcherAfterSessionHydration({
            remoteRunning,
            runtimeStatus: runtime?.threadStatus,
            hasWatchController: Boolean(runtime?.watchController),
            hasSendController: Boolean(runtime?.sendController),
            hasResumeController: Boolean(runtime?.resumeController),
            keepActiveSessionWarm: shouldKeepActiveSessionWarm
          });
        if (shouldStartWatcher) {
          startSessionWatcher(this, targetSessionId);
        }
        void this.refreshSessionSubagents(targetSessionId).catch(() => null);
        if (perfEnabled) {
          chatPerf.recordDuration('chat_session_detail_load', performance.now() - perfStart, {
            sessionId: targetSessionId,
            desktopMode: isDesktopModeEnabled(),
            fetchMs: perfFetchMs === null ? null : Number(perfFetchMs.toFixed(1)),
            hydrateMs: perfHydrateMs === null ? null : Number(perfHydrateMs.toFixed(1)),
            foregroundSyncMs:
              perfForegroundSyncMs === null ? null : Number(perfForegroundSyncMs.toFixed(1)),
            reusedHydratedMessages: perfReusedHydratedMessages,
            remoteRunning: perfRemoteRunning,
            transcriptCount: perfTranscriptCount,
            eventCount: perfEventCount,
            roundCount: perfRoundCount,
            messageCount: Array.isArray(this.messages) ? this.messages.length : 0
          });
        }
        return sessionDetail;
      });
    },
    async loadOlderHistory(sessionId, options: { limit?: number; beforeId?: number } = {}) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetId) return [];
      const state = getHistoryState(targetId);
      if (state.loading || state.hasMore === false) return [];
      const perfEnabled = chatPerf.enabled();
      const perfStart = perfEnabled ? performance.now() : 0;
      const limit = normalizeHistoryPageLimit(options.limit ?? HISTORY_PAGE_LIMIT);
      const beforeIdRaw = options.beforeId ?? state.beforeId;
      const beforeId = normalizeHistoryBeforeId(beforeIdRaw);
      state.loading = true;
      try {
        const currentSessionMessages = resolveSessionMessageArray(
          this,
          targetId,
          resolveSessionKey(this.activeSessionId) === targetId ? this.messages : null
        );
        const existingIds = buildExistingHistoryIdSet(currentSessionMessages);
        let cursor = beforeId;
        let incomingCount = 0;
        let incomingHasMore = false;
        let incomingBeforeId: number | null = null;
        let deduped: Record<string, unknown>[] = [];
        let messagesForCursor = currentSessionMessages;
        const maxEmptyPages = 3;
        for (let emptyPageCount = 0; emptyPageCount <= maxEmptyPages; emptyPageCount += 1) {
          const params: { before_id?: number; limit: number; summary: boolean } = {
            limit,
            summary: true
          };
          if (cursor !== null) {
            params.before_id = cursor;
          }
          const { data } = await getSessionHistoryPage(targetId, params);
          const payload = data?.data || {};
          const page = readHistoryBackfillPage(payload);
          incomingCount += page.transcript.length;
          incomingHasMore = page.hasMore;
          incomingBeforeId = page.beforeId;
          const pageDeduped = collectDedupedHistoryBackfillPage(page.transcript, existingIds);
          if (pageDeduped.length > 0) {
            deduped = prependHistoryBackfillPage(deduped, pageDeduped);
          }
          if (pageDeduped.length > 0 || !incomingHasMore) {
            break;
          }
          const nextCursor = incomingBeforeId;
          if (nextCursor === null || nextCursor === cursor) {
            break;
          }
          cursor = nextCursor;
        }
        if (deduped.length > 0) {
          const nextMessages = [...deduped, ...currentSessionMessages];
          messagesForCursor = nextMessages;
          const nextLimit = Math.min(
            Number(state.windowLimit || resolveSessionDetailMessageLimit(isDesktopModeEnabled())) + deduped.length,
            resolveMessageWindowMax(isDesktopModeEnabled())
          );
          state.windowLimit = nextLimit;
          if (resolveSessionKey(this.activeSessionId) === targetId) {
            const syncedMessages = replaceMessageArrayKeepingReference(
              this.messages,
              nextMessages
            );
            this.messages = syncedMessages;
            notifySessionSnapshot(this, targetId, syncedMessages, true, {
              skipWindowing: true
            });
          } else {
            cacheSessionMessages(targetId, nextMessages);
          }
        }
        state.beforeId = incomingBeforeId ?? findOldestHistoryId(messagesForCursor);
        state.hasMore = Boolean(incomingHasMore) && Boolean(state.beforeId);
        if (perfEnabled) {
          chatPerf.recordDuration('chat_history_load', performance.now() - perfStart, {
            sessionId: targetId,
            incoming: incomingCount,
            appended: deduped.length,
            hasMore: state.hasMore
          });
        }
        return deduped;
      } catch (error) {
        if (perfEnabled) {
          chatPerf.count('chat_history_load_failed', 1, { sessionId: targetId });
        }
        return [];
      } finally {
        state.loading = false;
      }
    },
};
