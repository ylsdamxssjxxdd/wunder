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

import { clearSessionCommandSessions, removeDemoChatSession, sortSessionsByActivity, syncDemoChatCache } from './chatDemoPanels';
import { DEFAULT_AGENT_KEY, applyMainSession, goalSessionIdFromPayload, persistAgentSession, resolvePersistedSessionId, writeSessionGoalState } from './chatPersist';
import { clearSessionWatcher, setSessionLoading } from './chatRuntimeControls';
import { clearSessionEventsSnapshot, filterSessionsByAgent, resolveSessionKey, sessionDetailPrefetchInFlight, sessionDetailWarmState, sessionHistoryState, sessionMessages, sessionRuntime, sessionSubagentsCache, sessionSubagentsInFlight, writeSessionListCache } from './chatRuntimeState';
import { clearChatSnapshot } from './chatSnapshot';
import { abortResumeStream, abortSendStream, startSessionWatcher } from './chatWatcher';
import { sessionWorkflowState } from './chatWorkflowHydration';

export const chatSessionMutationActions = {
    async renameSession(sessionId, title) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      const nextTitle = String(title || '').trim();
      if (!targetId || !nextTitle) return null;
      const { data } = await renameSessionApi(targetId, { title: nextTitle });
      const updated = data?.data || null;
      const index = this.sessions.findIndex((item) => resolveSessionKey(item?.id) === targetId);
      if (index >= 0) {
        const previous = this.sessions[index] || {};
        this.sessions[index] = {
          ...previous,
          ...(updated && typeof updated === 'object' ? updated : {}),
          id: targetId,
          title: String((updated && updated.title) || nextTitle).trim() || nextTitle
        };
        this.sessions = sortSessionsByActivity(this.sessions);
        const targetAgentId = String(this.sessions[index]?.agent_id || '').trim();
        writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
        syncDemoChatCache({ sessions: this.sessions });
      }
      return updated;
    },
    async archiveSession(sessionId) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetId) return null;
      if (resolveSessionKey(targetId) === resolveSessionKey(this.activeSessionId)) {
        clearSessionWatcher();
      }
      const targetSession = this.sessions.find((item) => resolveSessionKey(item?.id) === targetId) || null;
      const targetAgentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      abortResumeStream(targetId);
      abortSendStream(targetId);
      setSessionLoading(this, targetId, false);
      this.clearPendingApprovals({ sessionId: targetId });
      sessionRuntime.delete(resolveSessionKey(targetId));
      sessionMessages.delete(resolveSessionKey(targetId));
      clearSessionEventsSnapshot(targetId);
      sessionDetailWarmState.delete(resolveSessionKey(targetId));
      sessionDetailPrefetchInFlight.delete(resolveSessionKey(targetId));
      sessionSubagentsInFlight.delete(resolveSessionKey(targetId));
      sessionSubagentsCache.delete(resolveSessionKey(targetId));
      sessionHistoryState.delete(resolveSessionKey(targetId));
      const { data } = await archiveSessionApi(targetId);
      const archived = data?.data || null;
      this.sessions = this.sessions.filter((item) => resolveSessionKey(item?.id) !== targetId);
      if (targetSession?.is_main) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        const apiAgentId = targetAgentId || DEFAULT_AGENT_KEY;
        if (fallback) {
          await setDefaultSession(apiAgentId, { session_id: fallback.id });
          this.sessions = applyMainSession(this.sessions, targetAgentId, fallback.id);
          persistAgentSession(targetAgentId, fallback.id);
        } else {
          this.sessions = applyMainSession(this.sessions, targetAgentId, '');
          persistAgentSession(targetAgentId, '');
        }
      }
      sessionWorkflowState.delete(String(targetId));
      removeDemoChatSession(targetId);
      clearChatSnapshot(targetId);
      if (resolvePersistedSessionId(targetAgentId) === targetId) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        persistAgentSession(targetAgentId, fallback?.id || '');
      }
      writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
      if (this.activeSessionId === targetId) {
        const nextSession = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        if (nextSession) {
          await this.loadSessionDetail(nextSession.id);
        } else {
          this.openDraftSession({ agent_id: targetAgentId });
        }
      }
      return archived;
    },
    async restoreSession(sessionId) {
      const targetId = resolveSessionKey(sessionId);
      if (!targetId) return null;
      const { data } = await restoreSessionApi(targetId);
      const restored = data?.data || null;
      if (!restored || typeof restored !== 'object') {
        return restored;
      }
      const resolvedId = resolveSessionKey(restored.id || targetId);
      if (!resolvedId) {
        return restored;
      }
      const index = this.sessions.findIndex((item) => resolveSessionKey(item?.id) === resolvedId);
      if (index >= 0) {
        this.sessions[index] = { ...this.sessions[index], ...restored, id: resolvedId };
      } else {
        this.sessions.unshift({ ...restored, id: resolvedId });
      }
      const restoredAgentId = String(restored.agent_id || '').trim();
      if (restored?.is_main) {
        this.sessions = applyMainSession(this.sessions, restoredAgentId, resolvedId);
        persistAgentSession(restoredAgentId, resolvedId);
      }
      this.sessions = sortSessionsByActivity(this.sessions);
      writeSessionListCache(restoredAgentId, filterSessionsByAgent(restoredAgentId, this.sessions));
      syncDemoChatCache({ sessions: this.sessions });
      return restored;
    },
    async deleteSession(sessionId) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return;
      if (resolveSessionKey(targetId) === resolveSessionKey(this.activeSessionId)) {
        clearSessionWatcher();
      }
      const targetSession = this.sessions.find((item) => item.id === targetId) || null;
      const targetAgentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      abortResumeStream(targetId);
      abortSendStream(targetId);
      setSessionLoading(this, targetId, false);
      this.clearPendingApprovals({ sessionId: targetId });
      sessionRuntime.delete(resolveSessionKey(targetId));
      sessionMessages.delete(resolveSessionKey(targetId));
      clearSessionEventsSnapshot(targetId);
      sessionDetailWarmState.delete(resolveSessionKey(targetId));
      sessionDetailPrefetchInFlight.delete(resolveSessionKey(targetId));
      sessionSubagentsInFlight.delete(resolveSessionKey(targetId));
      sessionSubagentsCache.delete(resolveSessionKey(targetId));
      sessionHistoryState.delete(resolveSessionKey(targetId));
      clearSessionCommandSessions(targetId);
      await deleteSessionApi(targetId);
      this.sessions = this.sessions.filter((item) => item.id !== targetId);
      if (targetSession?.is_main) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        const apiAgentId = targetAgentId || DEFAULT_AGENT_KEY;
        if (fallback) {
          await setDefaultSession(apiAgentId, { session_id: fallback.id });
          this.sessions = applyMainSession(this.sessions, targetAgentId, fallback.id);
          persistAgentSession(targetAgentId, fallback.id);
        } else {
          this.sessions = applyMainSession(this.sessions, targetAgentId, '');
          persistAgentSession(targetAgentId, '');
        }
      }
      sessionWorkflowState.delete(String(targetId));
      clearSessionCommandSessions(targetId);
      removeDemoChatSession(targetId);
      clearChatSnapshot(targetId);
      if (resolvePersistedSessionId(targetAgentId) === targetId) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        persistAgentSession(targetAgentId, fallback?.id || '');
      }
      writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
      if (this.activeSessionId === targetId) {
        const nextSession = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        if (nextSession) {
          await this.loadSessionDetail(nextSession.id);
        } else {
          this.openDraftSession({ agent_id: targetAgentId });
        }
      }
    },
    async updateSessionTools(sessionId, toolOverrides = []) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return null;
      const payload = {
        tool_overrides: Array.isArray(toolOverrides) ? toolOverrides : []
      };
      const { data } = await updateSessionToolsApi(targetId, payload);
      const overrides = data?.data?.tool_overrides || [];
      const index = this.sessions.findIndex((item) => item.id === targetId);
      if (index >= 0) {
        this.sessions[index] = {
          ...this.sessions[index],
          tool_overrides: overrides
        };
      }
      return overrides;
    },
    syncSessionGoal(sessionId, goal = null) {
      const targetId = resolveSessionKey(sessionId) || goalSessionIdFromPayload(goal);
      if (!targetId) return null;
      return writeSessionGoalState(this, targetId, goal, { clear: !goal });
    },
    async refreshSessionGoal(sessionId = null) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetId) return null;
      const { data } = await getSessionGoal(targetId);
      return this.syncSessionGoal(targetId, data?.data?.goal ?? null);
    },
    async setSessionGoal(sessionId = null, payload: Record<string, unknown> = {}) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetId) {
        throw new Error(t('chat.command.goalMissingSession'));
      }
      const { data } = await setSessionGoalApi(targetId, payload || {});
      const goal = this.syncSessionGoal(targetId, data?.data?.goal ?? null);
      if (data?.data?.continuation?.should_start === true) {
        startSessionWatcher(this, targetId);
      }
      return {
        goal,
        continuation: data?.data?.continuation ?? null
      };
    },
};
