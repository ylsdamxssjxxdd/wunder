import {threadViewSlots} from '@/realtime/chat/threadViewStore';
import { prefetchThreadView } from './threadViewLoader';

import { isSessionUnavailable } from './chatSessionAvailability';

import { listSessions } from '@/api/chat';

import { materializeChatRuntimeMessages } from '@/realtime/chat/chatRuntimeRenderAdapter';

import { normalizeInquiryPanelState, normalizeInquiryPanelStatus, sortSessionsByActivity, syncDemoChatCache } from './chatDemoPanels';

import { DEFAULT_AGENT_KEY, patchSessionRuntimeFields, persistAgentSession, readChatPersistState, resolvePersistedSessionId } from './chatPersist';

import { applyCanonicalSessionEventsSnapshot, cloneSessionList, filterSessionsByAgent, getSessionMessages, isReusableFreshSession, loadSessionWorkflowEventsSnapshot, normalizeThreadControlSession, readSessionListCache, resolveInitialSessionIdFromList, resolveSessionKey, resolveSessionListCacheKey, sessionListCacheInFlight, syncChatRuntimeProjectionFromSnapshot, writeSessionListCache } from './chatRuntimeState';
import { readChatSnapshot, scheduleChatSnapshot } from './chatSnapshot';
import { resolveGreetingContent } from './chatStats';

import { InquiryPanelPatch, ListSessionsByStatusOptions } from './chatTypes';

const workflowHistoryHydrationInFlight = new Map<string, Promise<void>>();

const resolveWorkflowHistoryRoundRange = (messages: unknown): { from: number; to: number } | null => {
  if (!Array.isArray(messages)) return null;
  const rounds = messages
    .map((message) => {
      const record = message as Record<string, unknown>;
      const explicit = Number.parseInt(
        String(record?.user_round ?? record?.userRound ?? record?.user_turn_index ??
          record?.userTurnIndex ?? record?.stream_round ?? record?.streamRound ?? ''),
        10
      );
      if (Number.isFinite(explicit) && explicit > 0) return explicit;
      // Canonical projections use generated ids after hydration. Preserve the
      // numeric round encoded in those ids so refresh can still scope replay.
      const ids = [record?.user_turn_id, record?.userTurnId, record?.user_turn, record?.userTurn,
        record?.model_turn_id, record?.modelTurnId];
      for (const value of ids) {
        const match = String(value ?? '').match(/(?:user-turn:[^:]+:round:|:user:)(\d+)/i);
        if (match) {
          const parsed = Number.parseInt(match[1], 10);
          if (Number.isFinite(parsed) && parsed > 0) return parsed;
        }
      }
      return null;
    })
    .filter((value): value is number => Number.isFinite(value) && value > 0);
  if (rounds.length === 0) return null;
  return { from: Math.min(...rounds), to: Math.max(...rounds) };
};

export const chatCacheActions = {
    getPersistedState() {
      return readChatPersistState();
    },
    getLastSessionId(agentId) {
      return resolvePersistedSessionId(agentId);
    },
    setLastSessionId(agentId, sessionId) {
      persistAgentSession(agentId, sessionId);
    },
    syncSessionSummary(session, options: { agentId?: string; remember?: boolean } = {}) {
      const normalized = normalizeThreadControlSession(session);
      if (!normalized) return null;
      const requestedAgentId = String(options.agentId ?? '').trim();
      const fallbackAgentId = requestedAgentId === DEFAULT_AGENT_KEY ? '' : requestedAgentId;
      const targetSessionId = resolveSessionKey(normalized.id);
      const targetAgentId = String(normalized.agent_id || fallbackAgentId).trim();
      if (!targetSessionId) return null;
      if (isSessionUnavailable(this, targetSessionId)) return null;
      const nextSession: Record<string, unknown> = {
        ...(session as Record<string, unknown>),
        ...normalized,
        id: targetSessionId,
        agent_id: targetAgentId
      };
      const patchedSession = patchSessionRuntimeFields(nextSession) as Record<string, unknown>;
      const targetIndex = this.sessions.findIndex((item) => resolveSessionKey(item?.id) === targetSessionId);
      if (targetIndex >= 0) {
        this.sessions[targetIndex] = {
          ...this.sessions[targetIndex],
          ...patchedSession
        };
      } else {
        this.sessions.unshift(patchedSession);
      }
      this.sessions = sortSessionsByActivity(this.sessions);
      writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
      if (options.remember === true) {
        persistAgentSession(targetAgentId, targetSessionId);
      }
      syncDemoChatCache({ sessions: this.sessions });
      return patchedSession;
    },
    hasSessionMessages(sessionId) {
      const cached = getSessionMessages(sessionId);
      return Array.isArray(cached) && cached.length > 0;
    },
    getCachedSessionMessages(sessionId) {
      const targetId = resolveSessionKey(sessionId);
      if (!targetId) return [];
      const view=threadViewSlots(targetId);if(view!==null)return view.flatMap(s=>[s.user,s.assistant]);
      const _projectionVersion = this.runtimeProjectionVersion;
      const projected = materializeChatRuntimeMessages(this.runtimeProjection, targetId);
      if (projected.length > 0) {
        return projected;
      }
      const cached = getSessionMessages(targetId);
      const cachedMessages = Array.isArray(cached) ? cached : [];
      const activeSessionId = resolveSessionKey(this.activeSessionId);
      if (activeSessionId === targetId && Array.isArray(this.messages) && this.messages.length > 0) {
        if (cachedMessages.length > 0) {
          const resolveMessageTime = (message: Record<string, unknown> | null | undefined) => {
            const raw = message?.created_at ?? message?.createdAt ?? message?.updated_at ?? message?.updatedAt ?? message?.time;
            const numeric = typeof raw === 'number' ? raw : Date.parse(String(raw || ''));
            return Number.isFinite(numeric) ? numeric : 0;
          };
          const activeLatest = Math.max(
            ...this.messages.map((message) => resolveMessageTime(message as Record<string, unknown>))
          );
          const cachedLatest = Math.max(
            ...cachedMessages.map((message) => resolveMessageTime(message as Record<string, unknown>))
          );
          if (cachedLatest > activeLatest || (cachedLatest === activeLatest && cachedMessages.length > this.messages.length)) {
            return cachedMessages;
          }
        }
        return this.messages;
      }
      return cachedMessages;
    },
    async hydrateSessionWorkflowHistory(sessionId, sourceMessages = null) {
      const targetId = resolveSessionKey(sessionId);
      if(threadViewSlots(targetId)!==null)return;
      // Prefer the foreground snapshot when supplied; a warm session cache
      // can lag behind the transcript that just finished loading.
      const cachedTranscript = getSessionMessages(targetId);
      const transcript = Array.isArray(sourceMessages) && sourceMessages.length > 0
        ? sourceMessages
        : cachedTranscript;
      const range = resolveWorkflowHistoryRoundRange(
        Array.isArray(sourceMessages) ? sourceMessages : transcript
      );
      if (!targetId) return;
      const key = range
        ? `${targetId}:${range.from}:${range.to}`
        : `${targetId}:all`;
      const existing = workflowHistoryHydrationInFlight.get(key);
      if (existing) return existing;
      // The API excludes output deltas and is scoped to the visible history rounds.
      const task = loadSessionWorkflowEventsSnapshot(targetId, {
        ...(range
          ? { fromUserRound: range.from, toUserRound: range.to }
          : {})
      }).then((payload) => {
        if (!payload || !Array.isArray(payload.rounds) || payload.rounds.length === 0) return;
        // A prefetched session has no projection until workflow hydration starts.
        // Seed it from the full transcript so cards augment their real bubbles.
        if (!this.runtimeProjection?.sessions?.[targetId] && Array.isArray(transcript) && transcript.length > 0) {
          syncChatRuntimeProjectionFromSnapshot(this, targetId, transcript, {
            immediate: true,
            loading: false,
            running: false,
            authoritative: true
          });
        }
        applyCanonicalSessionEventsSnapshot(this, targetId, payload, {
          phase: 'history-workflow',
          includeRuntime: false
        });
      }).catch(() => undefined).finally(() => {
        if (workflowHistoryHydrationInFlight.get(key) === task) {
          workflowHistoryHydrationInFlight.delete(key);
        }
      });
      workflowHistoryHydrationInFlight.set(key, task);
      return task;
    },
    getCachedSessions(agentId) {
      const cached = readSessionListCache(agentId);
      if (cached) return cached;
      return filterSessionsByAgent(agentId, this.sessions);
    },
    resolveReusableFreshSessionId(agentId, options: { activeOnly?: boolean } = {}) {
      const requestedAgentId = String(agentId ?? '').trim();
      const normalizedAgentId =
        requestedAgentId === DEFAULT_AGENT_KEY ? '' : requestedAgentId;
      const activeSessionId = resolveSessionKey(this.activeSessionId);
      const activeOnly = options.activeOnly === true;
      const sessions = filterSessionsByAgent(normalizedAgentId, this.sessions);
      for (const session of sessions) {
        const sessionId = resolveSessionKey(session?.id);
        if (!sessionId) continue;
        if (this.loadingBySession?.[sessionId]) continue;
        if (activeOnly && sessionId !== activeSessionId) continue;
        const fallbackMessages = sessionId === activeSessionId ? this.messages : null;
        if (isReusableFreshSession(session, fallbackMessages)) {
          return sessionId;
        }
      }
      return '';
    },
    resolveInitialSessionId(agentId, sourceSessions = null) {
      const targetSessions = Array.isArray(sourceSessions) ? sourceSessions : this.getCachedSessions(agentId);
      return resolveInitialSessionIdFromList(agentId, targetSessions);
    },
    async prefetchAgentSessions(agentId) {
      const normalizedAgentId = String(agentId ?? '').trim();
      const cached = readSessionListCache(normalizedAgentId);
      if (cached) {
        return cached;
      }
      const cacheKey = resolveSessionListCacheKey(normalizedAgentId);
      const inFlight = sessionListCacheInFlight.get(cacheKey);
      if (inFlight) {
        return inFlight;
      }
      const request = (async () => {
        const params = { agent_id: normalizedAgentId };
        const { data } = await listSessions(params);
        const sessions = sortSessionsByActivity(data?.data?.items || []);
        writeSessionListCache(normalizedAgentId, sessions);
        return cloneSessionList(sessions);
      })().finally(() => {
        sessionListCacheInFlight.delete(cacheKey);
      });
      sessionListCacheInFlight.set(cacheKey, request);
      return request;
    },
    async listSessionsByStatus(options: ListSessionsByStatusOptions = {}) {
      const params: { agent_id?: string; status?: string } = {};
      if (Object.prototype.hasOwnProperty.call(options, 'agent_id')) {
        params.agent_id = String(options.agent_id ?? '');
      }
      const status = String(options.status || '').trim().toLowerCase();
      if (status === 'active' || status === 'archived' || status === 'all') {
        params.status = status;
      }
      const { data } = await listSessions(Object.keys(params).length ? params : undefined);
      return sortSessionsByActivity(data?.data?.items || []);
    },
    async preloadSessionDetail(sessionId, options: { force?: boolean; syncActive?: boolean } = {}) {
      const key=resolveSessionKey(sessionId);if(!key||isSessionUnavailable(this,key))return null;
      if(options.syncActive!==false&&this.activeSessionId===key)return this.loadSessionDetail(key);
      return prefetchThreadView(key);
    },
    getSnapshotForSession(sessionId) {
      const snapshot = readChatSnapshot();
      if (!snapshot || snapshot.sessionId !== String(sessionId || '')) {
        return null;
      }
      return snapshot;
    },
    scheduleSnapshot(immediate = false) {
      scheduleChatSnapshot(this, immediate);
    },
    setGreetingOverride(content) {
      const next = String(content || '').trim();
      this.greetingOverride = next;
      const greetingIndex = this.messages.findIndex((message) => message?.isGreeting);
      if (greetingIndex < 0) return;
      const greetingText = resolveGreetingContent(next);
      if (this.messages[greetingIndex].content !== greetingText) {
        this.messages[greetingIndex].content = greetingText;
        this.scheduleSnapshot(true);
      }
    },
    resolveInquiryPanel(message, patch: InquiryPanelPatch = {}) {
      if (!message || message.role !== 'assistant') return;
      const panel = normalizeInquiryPanelState(message.questionPanel);
      if (!panel) return;
      const status = normalizeInquiryPanelStatus(patch.status ?? panel.status);
      message.questionPanel = {
        ...panel,
        status,
        selected: Array.isArray(patch.selected) ? patch.selected : panel.selected,
        answers: patch.answers ?? panel.answers
      };
      // A resolved panel no longer owns the assistant turn.  The durable
      // transition follows through the new user submission, but settle the
      // local projection immediately so its status label cannot keep saying
      // “inquiry” while the next turn is already running.
      if (['answered', 'dismissed'].includes(status) &&
          ['waiting', 'waiting_input', 'waiting_user_input'].includes(String(message.status || '').trim().toLowerCase())) {
        message.status = 'completed';
        message.state = 'final';
        message.final = true;
        message.streaming = false;
        message.workflowStreaming = false;
        message.reasoningStreaming = false;
        message.stream_incomplete = false;
      }
      this.scheduleSnapshot(true);
    },
};
