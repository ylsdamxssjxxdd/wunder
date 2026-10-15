import { openThreadView, olderThreadView, newerThreadView } from './threadViewLoader';
// AI生成

import { sessionCatalogCheckIds, sessionCatalogCandidateIds, mergeSessionCatalogPage, cacheSessionCatalog } from './chatSessionCatalog';
import { isSessionUnavailable } from './chatSessionAvailability';

import { createSession, listSessions } from '@/api/chat';

import { chatDebugLog } from '@/utils/chatDebug';

import { syncDemoChatCache } from './chatDemoPanels';
import { ensureGreetingMessage } from './chatGreeting';

import { patchSessionRuntimeFields, persistActiveSession, persistDraftSession, syncGoalFromSessionRecord, syncGoalsFromSessionList } from './chatPersist';
import { clearDraftSessionBootstrapMarkers, clearSessionWatcher, setSessionLoading } from './chatRuntimeControls';
import { applyLocalChatMessageRuntimeEvent, cacheSessionMessages, ensureRuntime, filterSessionsByAgent, getHistoryState, notifySessionSnapshot, readSessionListCacheEntry, resolveSessionKey, resolveSessionListCacheKey, sessionListCacheInFlight, touchSessionUpdatedAt, writeSessionListCache } from './chatRuntimeState';

import { buildMessage } from './chatStats';

import { ALL_SESSION_LIST_CACHE_KEY, LOAD_SESSIONS_BACKGROUND_REFRESH_MIN_AGE_MS, normalizeSessionListItems, resolveLoadSessionsCacheKey as resolveLoadSessionsCacheKeyBase } from './chatSessionListLoadCache';
import { AppendLocalMessageOptions, CreateSessionOptions, LoadSessionDetailOptions, LoadSessionsOptions, OpenDraftSessionOptions } from './chatTypes';
import { abortResumeStream } from './chatWatcher';
import { getSessionWorkflowState } from './chatWorkflowHydration';

const resolveLoadSessionsCacheKey = (agentId: string | null) =>
  resolveLoadSessionsCacheKeyBase(agentId, resolveSessionListCacheKey);

const readLoadSessionsCacheEntry = (agentId: string | null, maxAgeMs: number) =>
  agentId === null
    ? readSessionListCacheEntry(ALL_SESSION_LIST_CACHE_KEY, { maxAgeMs })
    : readSessionListCacheEntry(agentId, { maxAgeMs });

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
      if (options.goalCommand === true && normalizedRole === 'user') {
        message.goal_command = true;
        message.goalCommand = true;
      }
      if (options.manualCompactionCommand === true && normalizedRole === 'user') {
        message.manual_compaction_command = true;
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
            ...(options.goalCommand === true ? { goal_command: true, goalCommand: true } : {}),
            ...(options.manualCompactionCommand === true ? { manual_compaction_command: true } : {})
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
        options: { writeCache?: boolean; loadedAt?: number; unavailable_session_ids?: unknown; checkedIds?: string[]; total?: unknown; candidateIds?: string[] } = {}
      ) => {
        mergeSessionCatalogPage(this, { items, total: options.total, unavailable_session_ids: options.unavailable_session_ids }, options.checkedIds,
          options.candidateIds ? { candidateIds: options.candidateIds, offset: 0 } : undefined);
        this.sessionsLoadedAt = Number.isFinite(Number(options.loadedAt))
          ? Number(options.loadedAt)
          : Date.now();
        syncGoalsFromSessionList(this, this.sessions);
        if (options.writeCache !== false) {
          cacheSessionCatalog(this, requestedAgentId);
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
        const candidateIds = sessionCatalogCandidateIds(this, requestedAgentId);
        const checkedIds = sessionCatalogCheckIds(this, requestedAgentId);
        const { data } = await listSessions({ ...params, known_session_ids: checkedIds.join(',') });
        return applyLoadedSessions(data?.data?.items, source, {
          checkedIds, candidateIds, total: data?.data?.total, unavailable_session_ids: data?.data?.unavailable_session_ids
        });
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
      writeSessionListCache(session.agent_id, filterSessionsByAgent(session.agent_id, this.sessions));
      this.activeSessionId = session.id;
      this.draftAgentId = String(session.agent_id || '').trim();
      this.sessionOrderPromotion = {
        sessionId: String(session.id || '').trim(),
        agentId: String(session.agent_id || '').trim()
      };
      this.sessionOrderRevision += 1;
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
      await openThreadView(this,session.id);
      return session;
    },
    async loadSessionDetail(sessionId, options: LoadSessionDetailOptions = {}) {
      const key=resolveSessionKey(sessionId);
      if(!key || isSessionUnavailable(this,key))return null;
      return openThreadView(this,key,options);
    },
    async loadNewerHistory(sessionId: string) {
      return newerThreadView(this, resolveSessionKey(sessionId || this.activeSessionId));
    },
    async loadOlderHistory(sessionId, options: { limit?: number; beforeId?: number } = {}) {
      void options;
      return olderThreadView(this,resolveSessionKey(sessionId || this.activeSessionId));
    },
};
