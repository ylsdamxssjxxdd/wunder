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

import { buildWorkflowItem } from './chatDemoPanels';
import { clearSessionWatcher, setSessionLoading } from './chatRuntimeControls';
import { buildRuntimeDebugSnapshot, cacheSessionMessages, clearRuntimePendingManualCompaction, clearSessionEventsSnapshot, ensureRuntime, getSessionMessages, markRuntimePendingManualCompaction, notifySessionSnapshot, touchSessionUpdatedAt } from './chatRuntimeState';
import { chatPageLifecycle } from './chatSharedState';
import { buildMessage } from './chatStats';
import { abortResumeStream, buildPendingManualCompactionMarkerMessage, finalizeManualCompactionAsCancelled, finalizeManualCompactionAsRequestFailed, findRunningManualCompactionMarkerMessage, isAbortRequestError, startSessionWatcher } from './chatWatcher';
import { buildDetail, cloneCompactionDebugPayload, normalizeCompactionDebugText, summarizeCompactionWorkflowItemsForDebug } from './chatWorkflowHydration';

export const chatCompactionActions = {
    async compactSession(sessionId, payload: Record<string, unknown> = {}) {
      const targetId = String(sessionId || this.activeSessionId || '').trim();
      if (!targetId) {
        throw new Error(t('chat.command.compactMissingSession'));
      }
      {
        const activeSessionIdForManual = String(this.activeSessionId || '').trim();
        const runtimeForManual = ensureRuntime(targetId);
        const shouldWatchActiveSession = activeSessionIdForManual === targetId;
        const debugPayloadEnabled = isChatDebugEnabled();
        const targetMessages =
          shouldWatchActiveSession
            ? this.messages
            : getSessionMessages(targetId) || [];
        const now = Date.now();
        let compactionMessage = findRunningManualCompactionMarkerMessage(targetMessages);
        if (!compactionMessage) {
          compactionMessage = buildPendingManualCompactionMarkerMessage(now);
          targetMessages.push(compactionMessage);
          cacheSessionMessages(targetId, targetMessages);
          touchSessionUpdatedAt(this, targetId, now);
          if (shouldWatchActiveSession) {
            notifySessionSnapshot(this, targetId, targetMessages, true);
          }
          chatDebugLog('chat.compaction.manual', 'local-marker-created', {
            sessionId: targetId,
            messageCount: targetMessages.length,
            marker: summarizeCompactionWorkflowItemsForDebug(compactionMessage.workflowItems)
          });
        }
        clearSessionEventsSnapshot(targetId);
        chatDebugLog('chat.compaction.manual', 'start', {
          sessionId: targetId,
          activeSessionId: activeSessionIdForManual,
          shouldWatchActiveSession,
          debugPayloadEnabled,
          payload: cloneCompactionDebugPayload(payload, {}),
          runtime: buildRuntimeDebugSnapshot(runtimeForManual)
        });
        runtimeForManual.stopRequested = false;
        if (runtimeForManual.compactController) {
          runtimeForManual.compactController.abort();
        }
        runtimeForManual.compactController = new AbortController();
        if (shouldWatchActiveSession) {
          markRuntimePendingManualCompaction(runtimeForManual, targetId);
        }
        const compactControllerForManual = runtimeForManual?.compactController || null;
        if (
          shouldWatchActiveSession &&
          !runtimeForManual?.watchController &&
          !runtimeForManual?.sendController &&
          !runtimeForManual?.resumeController
        ) {
          startSessionWatcher(this, targetId);
        }
        setSessionLoading(this, targetId, true);
        try {
          const requestPayload = {
            ...(payload && typeof payload === 'object' ? payload : {}),
            ...(debugPayloadEnabled ? { debug_payload: true } : {})
          };
          const { data } = await compactSessionApi(targetId, requestPayload, {
            signal: compactControllerForManual?.signal
          });
          chatDebugLog('chat.compaction.manual', 'accepted', {
            sessionId: targetId,
            response:
              data?.data && typeof data.data === 'object'
                ? data.data
                : data ?? null
          });
          return data?.data?.message || data?.message || '';
        } catch (error) {
          if (isAbortRequestError(error)) {
            const abortReason = chatPageLifecycle.pageUnloading ? 'page-unload' : 'request-cancelled';
            if (!chatPageLifecycle.pageUnloading) {
              clearRuntimePendingManualCompaction(runtimeForManual, targetId, abortReason);
              finalizeManualCompactionAsCancelled(compactionMessage);
              cacheSessionMessages(targetId, targetMessages);
              touchSessionUpdatedAt(this, targetId, Date.now());
              if (shouldWatchActiveSession) {
                notifySessionSnapshot(this, targetId, targetMessages, true);
              }
            }
            chatDebugLog('chat.compaction.manual', abortReason, {
              sessionId: targetId,
              marker: summarizeCompactionWorkflowItemsForDebug(compactionMessage?.workflowItems),
              runtime: buildRuntimeDebugSnapshot(runtimeForManual)
            });
          } else {
            clearRuntimePendingManualCompaction(runtimeForManual, targetId, 'request-failed');
            finalizeManualCompactionAsRequestFailed(compactionMessage, error);
            cacheSessionMessages(targetId, targetMessages);
            touchSessionUpdatedAt(this, targetId, Date.now());
            if (shouldWatchActiveSession) {
              notifySessionSnapshot(this, targetId, targetMessages, true);
            }
            const detailText = String(
              error?.response?.data?.detail || error?.message || t('common.requestFailed')
            ).trim();
            chatDebugLog('chat.compaction.manual', 'request-failed', {
              sessionId: targetId,
              code: String(error?.response?.data?.code || error?.code || ''),
              message: normalizeCompactionDebugText(detailText),
              marker: summarizeCompactionWorkflowItemsForDebug(compactionMessage?.workflowItems)
            });
          }
          if (!chatPageLifecycle.pageUnloading) {
            setSessionLoading(this, targetId, false);
          }
          throw error;
        } finally {
          if (
            runtimeForManual &&
            runtimeForManual.compactController === compactControllerForManual
          ) {
            runtimeForManual.compactController = null;
          }
          chatDebugLog('chat.compaction.manual', 'finalize', {
            sessionId: targetId,
            shouldWatchActiveSession,
            runtime: buildRuntimeDebugSnapshot(runtimeForManual)
          });
        }
      }
      return '';
      /*
      const activeSessionId = String(this.activeSessionId || '').trim();
      const shouldResumeWatcher = activeSessionId === targetId;
      if (shouldResumeWatcher) {
        abortResumeStream(targetId);
        clearSessionWatcher();
      }
      const runtime = ensureRuntime(targetId);
      chatDebugLog('chat.compaction.manual', 'start', {
        sessionId: targetId,
        activeSessionId,
        shouldResumeWatcher,
        payload: cloneCompactionDebugPayload(payload, {}),
        runtime: buildRuntimeDebugSnapshot(runtime)
      });
      if (runtime) {
        runtime.stopRequested = false;
        if (runtime.compactController) {
          runtime.compactController.abort();
        }
        runtime.compactController = new AbortController();
      }
      const compactController = runtime?.compactController || null;
      const targetMessages =
        activeSessionId === targetId
          ? this.messages
          : getSessionMessages(targetId) || [];
      const now = Date.now();
      const workflowRef = `compaction:manual:${now}`;
      const progressDetail = {
        stage: 'compacting',
        summary: t('chat.workflow.compactionRunning'),
        trigger_mode: 'manual'
      };
      const compactionMessage = {
        ...buildMessage('assistant', '', now),
        workflowItems: [
          buildWorkflowItem(
            t('chat.workflow.compactionRunning'),
            buildDetail(progressDetail),
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
        stream_incomplete: true
      };
      targetMessages.push(compactionMessage);
      chatDebugLog('chat.compaction.manual', 'local-marker-created', {
        sessionId: targetId,
        workflowRef,
        messageCount: targetMessages.length
      });
      setSessionLoading(this, targetId, true);
      cacheSessionMessages(targetId, targetMessages);
      touchSessionUpdatedAt(this, targetId, now);
      notifySessionSnapshot(this, targetId, targetMessages, true);
      try {
        const { data } = await compactSessionApi(targetId, payload, {
          signal: compactController?.signal
        });
        const resultData =
          data?.data && typeof data.data === 'object' ? data.data : {};
        if (Array.isArray(compactionMessage.workflowItems) && compactionMessage.workflowItems.length > 0) {
          compactionMessage.workflowItems[0].status = 'completed';
          compactionMessage.workflowItems[0].detail = buildDetail({
            ...(resultData as Record<string, unknown>),
            status: 'done',
            trigger_mode: 'manual'
          });
          (compactionMessage.workflowItems[0] as Record<string, unknown>).eventType = 'compaction';
        }
        compactionMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.toolWorkflow.compaction.title'),
            buildDetail({
              ...(resultData as Record<string, unknown>),
              status: 'done',
              trigger_mode: 'manual'
            }),
            'completed',
            {
              isTool: true,
              eventType: 'compaction',
              toolName: '上下文压缩',
              toolCallId: workflowRef
            }
          )
        );
        compactionMessage.workflowItems.length = 1;
        compactionMessage.workflowStreaming = false;
        compactionMessage.reasoningStreaming = false;
        compactionMessage.stream_incomplete = false;
        chatDebugLog('chat.compaction.manual', 'request-success', {
          sessionId: targetId,
          workflowRef,
          result: cloneCompactionDebugPayload(resultData, {}),
          marker: summarizeCompactionWorkflowItemsForDebug(compactionMessage.workflowItems)
        });
        return data?.data?.message || data?.message || '';
      } catch (error) {
        if (isAbortRequestError(error)) {
          chatDebugLog('chat.compaction.manual', 'request-cancelled', {
            sessionId: targetId,
            workflowRef,
            runtime: buildRuntimeDebugSnapshot(runtime)
          });
          finalizeManualCompactionAsCancelled(compactionMessage);
          return '';
        }
        const detailText = String(
          error?.response?.data?.detail || error?.message || t('common.requestFailed')
        ).trim();
        const failedDetail = buildDetail({
          stage: 'context_overflow_recovery',
          status: 'failed',
          trigger_mode: 'manual',
          error_code: String(error?.response?.data?.code || 'MANUAL_COMPACTION_FAILED'),
          error_message: detailText
        });
        if (Array.isArray(compactionMessage.workflowItems) && compactionMessage.workflowItems.length > 0) {
          compactionMessage.workflowItems[0].status = 'failed';
          compactionMessage.workflowItems[0].detail = failedDetail;
          (compactionMessage.workflowItems[0] as Record<string, unknown>).eventType = 'compaction';
        }
        compactionMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.toolWorkflow.compaction.title'),
            failedDetail,
            'failed',
            {
              isTool: true,
              eventType: 'compaction',
              toolName: '上下文压缩',
              toolCallId: workflowRef
            }
          )
        );
        compactionMessage.workflowItems.length = 1;
        compactionMessage.workflowStreaming = false;
        compactionMessage.reasoningStreaming = false;
        compactionMessage.stream_incomplete = false;
        chatDebugLog('chat.compaction.manual', 'request-failed', {
          sessionId: targetId,
          workflowRef,
          code: String(error?.response?.data?.code || error?.code || ''),
          message: normalizeCompactionDebugText(detailText),
          marker: summarizeCompactionWorkflowItemsForDebug(compactionMessage.workflowItems)
        });
        throw error;
      } finally {
        if (runtime && runtime.compactController === compactController) {
          runtime.compactController = null;
        }
        setSessionLoading(this, targetId, false);
        cacheSessionMessages(targetId, targetMessages);
        touchSessionUpdatedAt(this, targetId, Date.now());
        notifySessionSnapshot(this, targetId, targetMessages, true);
        if (shouldResumeWatcher && String(this.activeSessionId || '').trim() === targetId) {
          startSessionWatcher(this, targetId);
        }
        chatDebugLog('chat.compaction.manual', 'finalize', {
          sessionId: targetId,
          workflowRef,
          shouldResumeWatcher,
          marker: summarizeCompactionWorkflowItemsForDebug(compactionMessage.workflowItems),
          runtime: buildRuntimeDebugSnapshot(runtime)
        });
      }
      */
    },
};
