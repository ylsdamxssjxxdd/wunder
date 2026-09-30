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
import { SESSION_SUBAGENTS_CACHE_TTL_MS, cacheSessionMessages, getSessionMessages, notifySessionSnapshot, resolveChatHttpStatus, resolveSessionKey, sessionSubagentsCache, sessionSubagentsInFlight, touchSessionUpdatedAt } from './chatRuntimeState';
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
    async submitMessageFeedback(sessionId, itemId, vote) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) return null;
      const targetItemId = String(itemId ?? '').trim();
      if (!targetItemId) return null;
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
              String(message?.item_id || '').trim() === targetItemId
          )
        : null;
      const existingFeedback = normalizeMessageFeedback(existing?.feedback);
      if (existingFeedback?.vote) {
        return existingFeedback;
      }

      let feedback = null;
      try {
        const { data } = await submitMessageFeedbackApi(targetSessionId, targetItemId, {
          vote: normalizedVote
        });
        feedback =
          normalizeMessageFeedback(data?.data?.feedback) ||
          normalizeMessageFeedback({ vote: normalizedVote, locked: true });
      } catch (error) {
        throw error;
      }
      if (!feedback) return null;

      const updated = Array.isArray(targetMessages)
        ? targetMessages.some((message) => {
            if (String(message?.item_id || '').trim() !== targetItemId) return false;
            message.feedback = { ...feedback, locked: true };
            return true;
          })
        : false;
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
