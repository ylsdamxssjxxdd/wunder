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

import { dismissStaleInquiryPanels, ensureGreetingMessage, hydrateSessionCommandSessions, normalizeInquiryPanelState, normalizeInquiryPanelStatus, sortSessionsByActivity, syncDemoChatCache } from './chatDemoPanels';
import { hydrateMessage } from './chatMessageHydration';
import { DEFAULT_AGENT_KEY, applyMainSession, patchSessionRuntimeFields, persistAgentSession, readChatPersistState, resolvePersistedSessionId, syncGoalFromSessionRecord } from './chatPersist';
import { resolveKnownSessionEventFloor, resolveMaterializedMessageEventId } from './chatRuntimeControls';
import { applyCanonicalSessionEventsSnapshot, applyHistoryMeta, applyMessageWindow, applySessionRuntimeSnapshot, buildSessionHydratedMessageVersion, cacheSessionDetailSnapshot, cacheSessionMessages, clearCompletedAssistantStreamingState, cloneSessionList, ensureRuntime, filterSessionsByAgent, getSessionMessages, hasCanonicalSessionTranscript, hasKnownSessionInStore, isReusableFreshSession, isSessionDetailWarm, isSessionUnavailableStatus, loadSessionEventsSnapshot, markSessionDetailWarm, mergeSessionProtectedRealtimeMessages, normalizeThreadControlSession, purgeUnavailableSession, readSessionHydratedMessageVersion, readSessionListCache, resolveCanonicalSessionTranscript, resolveChatHttpStatus, resolveInitialSessionIdFromList, resolveSessionKey, resolveSessionListCacheKey, sessionDetailPrefetchInFlight, sessionListCacheInFlight, writeSessionHydratedMessageVersion, writeSessionListCache } from './chatRuntimeState';
import { readChatSnapshot, scheduleChatSnapshot } from './chatSnapshot';
import { resolveGreetingContent } from './chatStats';
import { normalizeStreamEventId, updateRuntimeLastEventId, updateRuntimeRemoteLastEventId } from './chatStreamIds';
import { InquiryPanelPatch, ListSessionsByStatusOptions } from './chatTypes';
import { attachWorkflowEvents, buildSessionWorkflowState, getSessionWorkflowState } from './chatWorkflowHydration';

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
      if (patchedSession.is_main === true) {
        this.sessions = applyMainSession(this.sessions, targetAgentId, targetSessionId);
      }
      this.sessions = sortSessionsByActivity(this.sessions);
      writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
      if (options.remember === true || nextSession.is_main === true) {
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
      const cached = getSessionMessages(sessionId);
      return Array.isArray(cached) ? cached : [];
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
      const targetId = resolveSessionKey(sessionId);
      if (!targetId) return null;
      const force = options.force === true;
      const syncActive = options.syncActive !== false;
      if (!hasKnownSessionInStore(this, targetId)) {
        chatDebugLog('chat.store.preload', 'skip-unknown-session', {
          sessionId: targetId,
          force,
          syncActive
        });
        purgeUnavailableSession(this, targetId);
        return null;
      }
      const cachedMessages = getSessionMessages(targetId) || [];
      if (!force && isSessionDetailWarm(targetId) && cachedMessages.length) {
        chatDebugLog('chat.store.preload', 'warm-hit', {
          sessionId: targetId,
          force,
          syncActive,
          messageCount: cachedMessages.length
        });
        return this.sessions.find((session) => session.id === targetId) || null;
      }
      const inFlight = sessionDetailPrefetchInFlight.get(targetId);
      if (inFlight) {
        chatDebugLog('chat.store.preload', 'reuse-inflight', {
          sessionId: targetId,
          force,
          syncActive
        });
        return inFlight;
      }
      const request = (async () => {
        let sessionRes = null;
        let eventsPayload = null;
        const knownEventFloor = resolveKnownSessionEventFloor(targetId);
        chatDebugLog('chat.store.preload', 'fetch-start', {
          sessionId: targetId,
          force,
          syncActive,
          knownEventFloor,
          activeSessionId: resolveSessionKey(this.activeSessionId)
        });
        try {
          [sessionRes, eventsPayload] = await Promise.all([
            getSession(targetId),
            loadSessionEventsSnapshot(targetId, {
              minLastEventId: knownEventFloor
            }).catch((error) => {
              if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
                throw error;
              }
              return null;
            })
          ]);
        } catch (error) {
          if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
            chatDebugLog('chat.store.preload', 'session-unavailable', {
              sessionId: targetId
            });
            purgeUnavailableSession(this, targetId);
            return null;
          }
          chatDebugLog('chat.store.preload', 'fetch-error', {
            sessionId: targetId,
            error:
              String((error as { name?: unknown; message?: unknown })?.message || '').trim() ||
              String((error as { name?: unknown; message?: unknown })?.name || '').trim()
          });
          throw error;
        }
        const payload = sessionRes?.data;
        const sessionDetail = payload?.data || null;
        cacheSessionDetailSnapshot(targetId, sessionDetail);
        syncGoalFromSessionRecord(this, sessionDetail);
        const hydratedVersion = buildSessionHydratedMessageVersion(sessionDetail, eventsPayload);
        hydrateSessionCommandSessions(
          targetId,
          eventsPayload?.command_sessions ?? eventsPayload?.commandSessions
        );
        const runtime = ensureRuntime(targetId);
        applySessionRuntimeSnapshot(runtime, eventsPayload?.runtime);
        applyCanonicalSessionEventsSnapshot(this, targetId, eventsPayload, {
          phase: 'preload'
        });
        const remoteRunning = eventsPayload?.running === true;
        const remoteLastEventId = normalizeStreamEventId(
          eventsPayload?.last_event_id ?? eventsPayload?.lastEventId
        );
        updateRuntimeRemoteLastEventId(
          runtime,
          remoteLastEventId
        );
        const cachedHydratedMessages = dedupeAssistantMessages(getSessionMessages(targetId));
        const hasCanonicalTranscript = hasCanonicalSessionTranscript(sessionDetail);
        const canReuseHydratedMessages =
          !hasCanonicalTranscript &&
          !remoteRunning &&
          Array.isArray(cachedHydratedMessages) &&
          cachedHydratedMessages.length > 0 &&
          readSessionHydratedMessageVersion(targetId) === hydratedVersion;
        let messages = cachedHydratedMessages;
        if (canReuseHydratedMessages) {
          getSessionWorkflowState(targetId, { reset: true });
        } else {
          const rounds = eventsPayload?.rounds || [];
          const workflowState = buildSessionWorkflowState();
          const rawMessages = attachWorkflowEvents(
            resolveCanonicalSessionTranscript(sessionDetail),
            rounds
          );
          messages = rawMessages.map((message) => hydrateMessage(message, workflowState));
          if (!hasCanonicalTranscript) {
            messages = dedupeAssistantMessages(messages);
          }
        }
        dismissStaleInquiryPanels(messages);
        const greetingMessages = ensureGreetingMessage(messages, {
          createdAt: sessionDetail?.created_at,
          greeting: this.greetingOverride
        });
        if (remoteRunning) {
          mergeSessionProtectedRealtimeMessages(targetId, greetingMessages);
        }
        if (!remoteRunning) {
          clearCompletedAssistantStreamingState(greetingMessages);
        }
        applyHistoryMeta(targetId, sessionDetail, greetingMessages);
        applyMessageWindow(this, targetId, greetingMessages);
        cacheSessionMessages(targetId, greetingMessages);
        const shouldSyncActiveMessages =
          syncActive && resolveSessionKey(this.activeSessionId) === targetId && Array.isArray(this.messages);
        if (shouldSyncActiveMessages) {
          replaceMessageArrayKeepingReference(this.messages, greetingMessages);
        }
        updateRuntimeLastEventId(
          runtime,
          Math.max(resolveMaterializedMessageEventId(greetingMessages), remoteLastEventId || 0)
        );
        writeSessionHydratedMessageVersion(targetId, hydratedVersion);
        markSessionDetailWarm(targetId);
        chatDebugLog('chat.store.preload', 'fetch-complete', {
          sessionId: targetId,
          force,
          syncActive,
          remoteRunning,
          remoteLastEventId,
          messageCount: greetingMessages.length,
          reusedHydratedMessages: canReuseHydratedMessages,
          syncedActiveMessages: shouldSyncActiveMessages
        });
        void this.refreshSessionSubagents(targetId).catch(() => null);
        return sessionDetail;
      })().finally(() => {
        sessionDetailPrefetchInFlight.delete(targetId);
      });
      sessionDetailPrefetchInFlight.set(targetId, request);
      return request;
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
      message.questionPanel = {
        ...panel,
        status: normalizeInquiryPanelStatus(patch.status ?? panel.status),
        selected: Array.isArray(patch.selected) ? patch.selected : panel.selected
      };
      this.scheduleSnapshot(true);
    },
};
