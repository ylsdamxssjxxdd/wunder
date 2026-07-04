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
import type { ChatRuntimeProjection } from '@/realtime/chat/chatRuntimeTypes';
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

import { HISTORY_PAGE_LIMIT } from './chatRuntimeControls';
import { SESSION_SUBAGENTS_CACHE_TTL_MS, applyAssistantHistoryIdBackfill, applyMessageFeedbackByHistoryId, cacheSessionMessages, getSessionMessages, isAssistantFeedbackCandidate, notifySessionSnapshot, resolveChatHttpStatus, resolveSessionKey, resolveTerminableSubagentSessionIds, sessionSubagentsCache, sessionSubagentsInFlight, touchSessionUpdatedAt } from './chatRuntimeState';
import { attachSubagentsToMessages } from './chatStats';

export const chatSubagentFeedbackActions = {
    async refreshSessionSubagents(sessionId, options: { force?: boolean } = {}) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) return [];
      const force = options.force === true;
      if (!force) {
        const cached = sessionSubagentsCache.get(targetSessionId);
        if (cached && Number.isFinite(cached.cachedAt) && Date.now() - cached.cachedAt <= SESSION_SUBAGENTS_CACHE_TTL_MS) {
          const targetMessages =
            resolveSessionKey(this.activeSessionId) === targetSessionId
              ? this.messages
              : getSessionMessages(targetSessionId) || [];
          if (Array.isArray(targetMessages) && targetMessages.length > 0) {
            attachSubagentsToMessages(targetMessages, cached.items);
            cacheSessionMessages(targetSessionId, targetMessages);
          }
          return cached.items;
        }
      }
      const inFlight = sessionSubagentsInFlight.get(targetSessionId);
      if (inFlight) {
        return inFlight;
      }
      const request = getSessionSubagents(targetSessionId)
        .then(({ data }) => {
          const items = Array.isArray(data?.data?.items) ? data.data.items : [];
          sessionSubagentsCache.set(targetSessionId, {
            cachedAt: Date.now(),
            items
          });
          const targetMessages =
            resolveSessionKey(this.activeSessionId) === targetSessionId
              ? this.messages
              : getSessionMessages(targetSessionId) || [];
          if (Array.isArray(targetMessages) && targetMessages.length > 0) {
            attachSubagentsToMessages(targetMessages, items);
            cacheSessionMessages(targetSessionId, targetMessages);
            touchSessionUpdatedAt(this, targetSessionId, Date.now());
            notifySessionSnapshot(this, targetSessionId, targetMessages, true);
          }
          return items;
        })
        .finally(() => {
          sessionSubagentsInFlight.delete(targetSessionId);
        });
      sessionSubagentsInFlight.set(targetSessionId, request);
      return request;
    },
    async controlSubagent(sessionId, subagent, action = 'terminate') {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) return null;
      const sessionIdList = Array.isArray(subagent)
        ? subagent
        : [subagent?.session_id ?? subagent?.sessionId ?? subagent];
      const sessionIds = sessionIdList
        .map((value) => String(value || '').trim())
        .filter(Boolean);
      if (sessionIds.length === 0) return null;
      const { data } = await controlSessionSubagentsApi(targetSessionId, {
        action,
        session_ids: sessionIds
      });
      await this.refreshSessionSubagents(targetSessionId, { force: true });
      return data?.data || null;
    },
    async terminateSessionSubagentTree(sessionId, options: { force?: boolean } = {}) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) {
        return {
          terminatedSessionIds: [],
          failedSessionIds: []
        };
      }
      const visitedParents = new Set<string>();
      const relations: Array<{ parentSessionId: string; childSessionIds: string[] }> = [];
      const stack = [targetSessionId];
      while (stack.length > 0) {
        const parentSessionId = String(stack.pop() || '').trim();
        if (!parentSessionId || visitedParents.has(parentSessionId)) continue;
        visitedParents.add(parentSessionId);
        let items: unknown[] = [];
        try {
          const fetched = await this.refreshSessionSubagents(parentSessionId, { force: options.force === true });
          items = Array.isArray(fetched) ? fetched : [];
        } catch {
          items = [];
        }
        const childSessionIds = resolveTerminableSubagentSessionIds(items);
        if (!childSessionIds.length) continue;
        relations.push({ parentSessionId, childSessionIds });
        childSessionIds.forEach((childSessionId) => {
          if (!visitedParents.has(childSessionId)) {
            stack.push(childSessionId);
          }
        });
      }

      const terminatedSessionIds = new Set<string>();
      const failedSessionIds = new Set<string>();
      for (const relation of relations.reverse()) {
        try {
          const { data } = await controlSessionSubagentsApi(relation.parentSessionId, {
            action: 'terminate',
            session_ids: relation.childSessionIds
          });
          const payload = data?.data as Record<string, unknown> | null;
          const resultItems = Array.isArray(payload?.items) ? payload.items : [];
          const updatedIds = resolveTerminableSubagentSessionIds(resultItems);
          relation.childSessionIds.forEach((childSessionId) => {
            if (updatedIds.includes(childSessionId)) {
              failedSessionIds.add(childSessionId);
            } else {
              terminatedSessionIds.add(childSessionId);
            }
          });
        } catch {
          relation.childSessionIds.forEach((childSessionId) => {
            failedSessionIds.add(childSessionId);
          });
        } finally {
          await this.refreshSessionSubagents(relation.parentSessionId, { force: true }).catch(() => []);
        }
      }

      terminatedSessionIds.forEach((sessionKey) => {
        failedSessionIds.delete(sessionKey);
      });
      return {
        terminatedSessionIds: Array.from(terminatedSessionIds),
        failedSessionIds: Array.from(failedSessionIds)
      };
    },
    async ensureAssistantMessageHistoryId(sessionId, message = null) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) return 0;
      const directHistoryId = resolveMessageHistoryId(message);
      if (directHistoryId > 0) return directHistoryId;

      const isActiveSession = resolveSessionKey(this.activeSessionId) === targetSessionId;
      const targetMessages = isActiveSession
        ? this.messages
        : getSessionMessages(targetSessionId) || [];
      if (!Array.isArray(targetMessages) || targetMessages.length === 0) {
        return 0;
      }
      if (!targetMessages.some((item) => isAssistantFeedbackCandidate(item))) {
        return resolveMessageHistoryId(message);
      }

      try {
        const { data } = await getSessionHistoryPage(targetSessionId, {
          limit: Math.max(HISTORY_PAGE_LIMIT, 120)
        });
        const payload = data?.data || {};
        const incoming = Array.isArray(payload.transcript) ? payload.transcript : [];
        const updatedCount = applyAssistantHistoryIdBackfill(targetMessages, incoming);
        if (updatedCount > 0) {
          touchSessionUpdatedAt(this, targetSessionId, Date.now());
          notifySessionSnapshot(this, targetSessionId, targetMessages, true);
        }
      } catch (error) {
        // Best effort only: feedback remains optional if history backfill fails.
      }

      return resolveMessageHistoryId(message);
    },
    async submitMessageFeedback(sessionId, historyId, vote) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) return null;
      const targetHistoryId = Number.parseInt(String(historyId ?? ''), 10);
      if (!Number.isFinite(targetHistoryId) || targetHistoryId <= 0) return null;
      const normalizedVote = normalizeMessageFeedbackVote(vote);
      if (!normalizedVote) return null;

      const activeSessionId = resolveSessionKey(this.activeSessionId);
      const targetMessages =
        activeSessionId === targetSessionId
          ? this.messages
          : getSessionMessages(targetSessionId) || [];
      const existing = Array.isArray(targetMessages)
        ? targetMessages.find(
            (message) =>
              message?.role === 'assistant' &&
              resolveMessageHistoryId(message) === targetHistoryId
          )
        : null;
      const existingFeedback = normalizeMessageFeedback(existing?.feedback);
      if (existingFeedback?.vote) {
        return existingFeedback;
      }

      let feedback = null;
      try {
        const { data } = await submitMessageFeedbackApi(targetSessionId, targetHistoryId, {
          vote: normalizedVote
        });
        feedback =
          normalizeMessageFeedback(data?.data?.feedback) ||
          normalizeMessageFeedback({ vote: normalizedVote, locked: true });
      } catch (error) {
        if (resolveChatHttpStatus(error) === 409) {
          feedback =
            normalizeMessageFeedback(existing?.feedback) ||
            normalizeMessageFeedback({ vote: normalizedVote, locked: true });
        } else {
          throw error;
        }
      }
      if (!feedback) return null;

      const updated = applyMessageFeedbackByHistoryId(targetMessages, targetHistoryId, feedback);
      if (!updated) {
        return feedback;
      }
      touchSessionUpdatedAt(this, targetSessionId, Date.now());
      notifySessionSnapshot(this, targetSessionId, targetMessages, true);
      return feedback;
    },
    dismissPendingInquiryPanel() {
      for (let i = this.messages.length - 1; i >= 0; i -= 1) {
        const message = this.messages[i];
        if (message?.role !== 'assistant') continue;
        if (message?.questionPanel?.status !== 'pending') continue;
        this.resolveInquiryPanel(message, { status: 'dismissed' });
      }
    },
};
