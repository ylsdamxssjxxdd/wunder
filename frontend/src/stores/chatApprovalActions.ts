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

import { normalizeApprovalResultId, normalizePendingApproval, safeJsonParse } from './chatDemoPanels';
import { collectSnapshotApprovalEvents } from './chatApprovalSnapshot';
import { clearSessionWatcher } from './chatRuntimeControls';
import { resolveSessionKey, syncSessionPendingApprovalRuntime } from './chatRuntimeState';
import { chatPageLifecycle } from './chatSharedState';
import { ApprovalDecision } from './chatTypes';
import { chatWsClient, resetChatRuntimeState } from './chatWatcher';

const resolveApprovalEventOrder = (record: Record<string, unknown>, fallback: number): number => {
  const outerData = record.data && typeof record.data === 'object' && !Array.isArray(record.data)
    ? record.data as Record<string, unknown>
    : {};
  const data = outerData.data && typeof outerData.data === 'object' && !Array.isArray(outerData.data)
    ? outerData.data as Record<string, unknown>
    : outerData;
  const value = record.event_seq ?? record.eventSeq ?? record.event_id ?? record.eventId ??
    outerData.event_seq ?? outerData.eventSeq ?? outerData.event_id ?? outerData.eventId ??
    data.event_seq ?? data.eventSeq ?? data.event_id ?? data.eventId;
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
};

export const chatApprovalActions = {
    markPageUnloading() {
      chatPageLifecycle.pageUnloading = true;
      clearSessionWatcher();
    },
    resetState() {
      chatPageLifecycle.pageUnloading = false;
      resetChatRuntimeState();
      this.$reset();
      // $reset restores the raw projection and its render clocks together.
      this.runtimeProjectionVersion = 0;
    },
    enqueueApprovalRequest(requestId, sessionId, payload) {
      const approval = normalizePendingApproval(payload, requestId, sessionId);
      if (!approval) return null;
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      const filtered = current.filter((item) => item?.approval_id !== approval.approval_id);
      this.pendingApprovals = [...filtered, approval];
      syncSessionPendingApprovalRuntime(this, approval.session_id);
      return approval;
    },
    restorePendingApprovals(sessionId, events) {
      const targetSessionId = resolveSessionKey(sessionId);
      const snapshotEvents = collectSnapshotApprovalEvents(events);
      if (!targetSessionId || !snapshotEvents) return false;
      const latestByApprovalId = new Map<string, {
        event: string;
        data: Record<string, unknown>;
        order: number;
      }>();
      for (const [index, item] of snapshotEvents.entries()) {
        const record = item && typeof item === 'object' ? item as Record<string, unknown> : {};
        const event = String(record.event ?? record.event_type ?? record.type ?? '').trim().toLowerCase();
        if (event !== 'approval_request' && event !== 'approval_result' && event !== 'approval_resolved') continue;
        const rawData = record.data;
        const outerData = rawData && typeof rawData === 'object' && !Array.isArray(rawData)
          ? rawData as Record<string, unknown>
          : typeof rawData === 'string'
            ? safeJsonParse(rawData) || {}
            : {};
        const data = outerData.data && typeof outerData.data === 'object' && !Array.isArray(outerData.data)
          ? outerData.data as Record<string, unknown>
          : outerData;
        const approvalId = String(data.approval_id ?? data.approvalId ?? '').trim();
        if (!approvalId) continue;
        const order = resolveApprovalEventOrder(record, index);
        const latest = latestByApprovalId.get(approvalId);
        if (!latest || order >= latest.order) {
          latestByApprovalId.set(approvalId, { event, data, order });
        }
      }
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      const next = current.filter((item) => resolveSessionKey(item?.session_id) !== targetSessionId);
      for (const { event, data } of latestByApprovalId.values()) {
        if (event !== 'approval_request') continue;
        const approval = normalizePendingApproval(data, data.request_id ?? data.requestId, targetSessionId);
        if (approval) next.push(approval);
      }
      const changed = next.length !== current.length || next.some((item, index) => item?.approval_id !== current[index]?.approval_id);
      if (changed) this.pendingApprovals = next;
      syncSessionPendingApprovalRuntime(this, targetSessionId);
      return changed;
    },
    resolveApprovalResult(payload) {
      const approvalId = normalizeApprovalResultId(payload);
      if (!approvalId) return false;
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      const resolvedSessions = new Set(
        current
          .filter((item) => item?.approval_id === approvalId)
          .map((item) => resolveSessionKey(item?.session_id))
          .filter(Boolean)
      );
      const next = current.filter((item) => item?.approval_id !== approvalId);
      const changed = next.length !== current.length;
      if (changed) {
        this.pendingApprovals = next;
        resolvedSessions.forEach((sessionId) => syncSessionPendingApprovalRuntime(this, sessionId));
      }
      return changed;
    },
    clearPendingApprovals(options: { sessionId?: string; requestId?: string } = {}) {
      const targetSessionId = String(options.sessionId || '').trim();
      const targetRequestId = String(options.requestId || '').trim();
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      if (!targetSessionId && !targetRequestId) {
        const sessionIds = Array.from(
          new Set(current.map((item) => resolveSessionKey(item?.session_id)).filter(Boolean))
        );
        this.pendingApprovals = [];
        sessionIds.forEach((sessionId) => syncSessionPendingApprovalRuntime(this, sessionId));
        return;
      }
      const resolvedSessions = new Set<string>();
      this.pendingApprovals = current.filter((item) => {
        if (!item) return false;
        if (targetSessionId && String(item.session_id || '').trim() !== targetSessionId) {
          return true;
        }
        if (targetRequestId && String(item.request_id || '').trim() !== targetRequestId) {
          return true;
        }
        const resolved = resolveSessionKey(item.session_id);
        if (resolved) {
          resolvedSessions.add(resolved);
        }
        return false;
      });
      resolvedSessions.forEach((sessionId) => syncSessionPendingApprovalRuntime(this, sessionId));
    },
    async respondApproval(decision: ApprovalDecision, approvalId = '') {
      const normalizedDecision = String(decision || '').trim().toLowerCase();
      const resolvedDecision: ApprovalDecision =
        normalizedDecision === 'approve_session'
          ? 'approve_session'
          : normalizedDecision === 'approve_once'
            ? 'approve_once'
            : 'deny';
      const targetApprovalId = String(
        approvalId || this.activeApproval?.approval_id || ''
      ).trim();
      if (!targetApprovalId) return false;
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      const target = current.find((item) => item?.approval_id === targetApprovalId);
      if (!target) return false;
      try {
        await chatWsClient.notify({
          type: 'approval',
          session_id: target.session_id,
          payload: {
            approval_id: targetApprovalId,
            decision: resolvedDecision
          }
        });
      } catch (error) {
        if (resolvedDecision !== 'deny') {
          throw error;
        }
      }
      this.pendingApprovals = current.filter((item) => item?.approval_id !== targetApprovalId);
      syncSessionPendingApprovalRuntime(this, target.session_id);
      return true;
    },
};
