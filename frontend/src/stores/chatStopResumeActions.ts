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

import { buildWorkflowItem, dismissStaleInquiryPanels, normalizeInquiryPanelState, safeJsonParse } from './chatDemoPanels';
import { applyGoalStreamEvent, writeSessionGoalState } from './chatPersist';
import { WATCH_USER_MESSAGE_DEDUP_MS, abortWatchStream, clearRuntimeResumeStreamState, clearSlowClientResume, insertWatchUserMessage, markRuntimeResumeStreamActivity, markRuntimeResumeStreamStarted, resolveHiddenInternalUserEvent, resolveMaterializedMessageEventId, resolveStreamFlushMsForMessages, setSessionLoading } from './chatRuntimeControls';
import { applySessionRuntimeEvent, cacheSessionMessages, captureRealtimeWorkflowMutationBaseline, clearSessionEventsSnapshot, ensureRuntime, getSessionMessages, handleThreadControlWorkflowEvent, logRealtimeWorkflowMutation, notifySessionSnapshot, protectRealtimeChannelMessage, refreshRuntimeStreamLifecycle, resolveSessionKey, resolveSessionMessageArray, syncSessionContextTokens, touchSessionUpdatedAt } from './chatRuntimeState';
import { chatPageLifecycle } from './chatSharedState';
import { buildMessage, clearAssistantRetryState, resetAssistantWaitingOutputPhase, resolveTimestampMs } from './chatStats';
import { assignStreamEventId, getRuntimeLastEventId, normalizeStreamEventId, updateRuntimeLastEventId, updateRuntimeRemoteLastEventId } from './chatStreamIds';
import { ResumeStreamOptions } from './chatTypes';
import { abortCompactRequest, abortResumeStream, abortSendStream, buildWsRequestId, chatWsClient, finalizeManualCompactionAsCancelled, scheduleSlowClientResume, startSessionWatcher } from './chatWatcher';
import { getSessionWorkflowState, handleApprovalEvent, isTerminalLlmOutputPayload, isTerminalRuntimeStatus, isTerminalStreamEventType, resolveNormalizedStreamEventType, shouldTreatRuntimeEventAsTerminal } from './chatWorkflowHydration';
import { createWorkflowProcessor } from './chatWorkflowProcessor';

export const chatStopResumeActions = {
    async stopSessionActivity(
      sessionId = null,
      options: { terminateSubagents?: boolean } = {}
    ) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) {
        return false;
      }
      clearSessionEventsSnapshot(targetSessionId);
      const runtime = ensureRuntime(targetSessionId);
      if (runtime) {
        runtime.stopRequested = true;
      }
      abortSendStream(targetSessionId);
      abortCompactRequest(targetSessionId);
      let cancelled = false;
      try {
        const { data } = await cancelMessageStream(targetSessionId);
        cancelled = Boolean(data?.data?.cancelled);
        if (data?.data?.goal_cleared === true) {
          writeSessionGoalState(this, targetSessionId, null, { clear: true });
          cancelled = true;
        }
      } catch (error) {
        // Ignore cancel API failures; local stop behavior still applies.
      }
      const targetMessages =
        String(this.activeSessionId || '').trim() === targetSessionId
          ? this.messages
          : getSessionMessages(targetSessionId);
      clearSupersededPendingAssistantMessages(targetMessages);
      const pendingAssistant = findPendingAssistantMessage(targetMessages);
      if (pendingAssistant) {
        if (isCompactionMarkerAssistantMessage(pendingAssistant)) {
          finalizeManualCompactionAsCancelled(pendingAssistant);
        } else {
          pendingAssistant.workflowStreaming = false;
          pendingAssistant.reasoningStreaming = false;
          pendingAssistant.stream_incomplete = false;
          pendingAssistant.resume_available = false;
          clearAssistantRetryState(pendingAssistant);
          if (!pendingAssistant.content) {
            pendingAssistant.content = t('chat.workflow.aborted');
          }
        }
        const panel = normalizeInquiryPanelState(pendingAssistant.questionPanel);
        if (panel && panel.status === 'pending') {
          pendingAssistant.questionPanel = { ...panel, status: 'dismissed' };
        }
        cancelled = true;
      }
      this.dismissPendingInquiryPanel();
      let terminatedSubagentCount = 0;
      if (options.terminateSubagents !== false) {
        const termination = await this.terminateSessionSubagentTree(targetSessionId, { force: true });
        terminatedSubagentCount = Array.isArray(termination?.terminatedSessionIds)
          ? termination.terminatedSessionIds.length
          : 0;
      }
      if (Array.isArray(targetMessages)) {
        cacheSessionMessages(targetSessionId, targetMessages);
        touchSessionUpdatedAt(this, targetSessionId, Date.now());
        notifySessionSnapshot(this, targetSessionId, targetMessages, true);
      }
      setSessionLoading(this, targetSessionId, false);
      return cancelled || terminatedSubagentCount > 0;
    },
    async stopStream() {
      return this.stopSessionActivity(this.activeSessionId, { terminateSubagents: true });
    },
    async resumeStream(sessionId, message, options: ResumeStreamOptions = {}) {
      const force = options.force === true;
      if (!message || (!message.stream_incomplete && !force)) return;
      abortWatchStream(sessionId);
      clearSessionEventsSnapshot(sessionId);
      setSessionLoading(this, sessionId, true);
      const perfEnabled = chatPerf.enabled();
      const perfStreamStart = perfEnabled ? performance.now() : 0;
      message.resume_available = false;
      message.slow_client = false;
      clearAssistantRetryState(message);
      message.workflowStreaming = true;
      message.stream_incomplete = true;
      resetAssistantWaitingOutputPhase(message);
      const sessionMessagesRef = resolveSessionMessageArray(this, sessionId, this.messages);
      cacheSessionMessages(sessionId, sessionMessagesRef);
      notifySessionSnapshot(this, sessionId, sessionMessagesRef);
      const workflowState = getSessionWorkflowState(sessionId);
      const processor = createWorkflowProcessor(
        message,
        workflowState,
        (immediate = false) => notifySessionSnapshot(this, sessionId, sessionMessagesRef, immediate),
        {
          streamFlushMs: resolveStreamFlushMsForMessages(sessionMessagesRef),
          sessionId,
          onThreadControl: (payload) => handleThreadControlWorkflowEvent(this, payload),
          onContextUsage: (contextTokens, contextTotalTokens) =>
            syncSessionContextTokens(this, sessionId, contextTokens, contextTotalTokens)
        }
      );
      abortResumeStream(sessionId);
      const runtime = ensureRuntime(sessionId);
      if (runtime) {
        clearSlowClientResume(runtime);
        const nextResumeRequestId = buildWsRequestId();
        runtime.resumeController = new AbortController();
        runtime.resumeRequestId = nextResumeRequestId;
        markRuntimeResumeStreamStarted(runtime);
        refreshRuntimeStreamLifecycle(runtime);
      }
      let aborted = false;
      let finalSeen = false;
      let errorSeen = false;
      let slowClientResumeAfterEventId = 0;
      let resumeRequestId = runtime?.resumeRequestId || '';
      const forcedEventId = options.afterEventId;
      const normalizedMessageEventId = normalizeStreamEventId(message.stream_event_id);
      const afterEventId = Number.isFinite(Number(forcedEventId))
        ? Number.parseInt(String(forcedEventId), 10)
        : normalizedMessageEventId;
      const resumeAfterEventId = Number.isFinite(afterEventId) ? Math.max(afterEventId, 0) : 0;
      let resumeLastEventId = Math.max(
        resolveMaterializedMessageEventId(sessionMessagesRef),
        getRuntimeLastEventId(runtime),
        resumeAfterEventId
      );
      try {
        const onEvent = (eventType, dataText, eventId) => {
          markRuntimeResumeStreamActivity(runtime);
          const payload = safeJsonParse(dataText);
          const approvalPayload = payload?.data ?? payload;
          const normalizedEventType = resolveNormalizedStreamEventType(eventType, payload);
          if (applyGoalStreamEvent(this, sessionId, normalizedEventType, approvalPayload)) {
            return;
          }
          if (perfEnabled) {
            chatPerf.count('chat_resume_event', 1, { eventType: normalizedEventType || eventType, sessionId });
          }
          if (normalizedEventType === 'heartbeat' || normalizedEventType === 'ping') {
            return;
          }
          handleApprovalEvent(
            this,
            normalizedEventType || eventType,
            approvalPayload,
            runtime?.resumeRequestId || '',
            sessionId
          );
          if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
            if (shouldTreatRuntimeEventAsTerminal(normalizedEventType, approvalPayload)) {
              const runtimeStatus = normalizeThreadRuntimeStatus(
                approvalPayload?.thread_status ?? approvalPayload?.status
              );
              if (runtimeStatus === 'system_error') {
                errorSeen = true;
              } else {
                finalSeen = true;
              }
            }
            updateRuntimeRemoteLastEventId(runtime, eventId);
            applySessionRuntimeEvent(this, sessionId, approvalPayload, normalizedEventType);
            return;
          }
          if (normalizedEventType === 'channel_message') {
            const eventTimestampMs = resolveTimestampMs(payload?.timestamp ?? approvalPayload?.timestamp);
            const channelRole = String(
              approvalPayload?.role ?? payload?.role ?? ''
            ).trim().toLowerCase();
            const channelContent = String(
              approvalPayload?.content ?? payload?.content ?? ''
            ).trim();
            const result = consumeChatWatchChannelMessage({
              messages: sessionMessagesRef,
              lastEventId: resumeLastEventId,
              eventId,
              eventTimestampMs,
              payload,
              data: approvalPayload,
              normalizeEventId: normalizeStreamEventId,
              buildMessage,
              assignStreamEventId,
              insertWatchUserMessage: (content, timestampMs, anchor, insertOptions) =>
                insertWatchUserMessage(
                  this,
                  sessionId,
                  sessionMessagesRef,
                  content,
                  timestampMs,
                  anchor,
                  insertOptions
                ),
              clearSupersededPendingAssistantMessages,
              dismissStaleInquiryPanels,
              touchUpdatedAt: (timestamp) => touchSessionUpdatedAt(this, sessionId, timestamp),
              notifySnapshot: (immediate = true) =>
                notifySessionSnapshot(this, sessionId, sessionMessagesRef, immediate),
              hiddenInternalUser: resolveHiddenInternalUserEvent(payload, approvalPayload),
              dedupeAssistantWindowMs: WATCH_USER_MESSAGE_DEDUP_MS
            });
            if (result.handled) {
              if (result.lastEventId > resumeLastEventId) {
                resumeLastEventId = result.lastEventId;
                updateRuntimeLastEventId(runtime, result.lastEventId);
                updateRuntimeRemoteLastEventId(runtime, result.lastEventId);
              }
              const normalizedEventId = normalizeStreamEventId(eventId);
              if (
                result.mutated &&
                normalizedEventId !== null &&
                (channelRole === 'user' || channelRole === 'assistant') &&
                channelContent
              ) {
                protectRealtimeChannelMessage(
                  sessionId,
                  sessionMessagesRef,
                  normalizedEventId,
                  channelRole,
                  channelContent,
                  eventTimestampMs,
                  resolveHiddenInternalUserEvent(payload, approvalPayload)
                );
              }
              return;
            }
          }
          assignStreamEventId(message, eventId);
          updateRuntimeLastEventId(runtime, eventId);
          updateRuntimeRemoteLastEventId(runtime, eventId);
          resumeLastEventId = Math.max(
            resumeLastEventId,
            normalizeStreamEventId(eventId) || 0
          );
          if (isTerminalStreamEventType(normalizedEventType)) {
            if (normalizedEventType === 'error' || normalizedEventType === 'queue_fail') {
              errorSeen = true;
            } else {
              finalSeen = true;
            }
          } else if (
            normalizedEventType === 'llm_output' &&
            isTerminalLlmOutputPayload(payload, approvalPayload)
          ) {
            finalSeen = true;
          } else if (
            normalizedEventType === 'slow_client' &&
            String(payload?.reason ?? payload?.data?.reason ?? '').trim() === 'queue_full_resume_required'
          ) {
            slowClientResumeAfterEventId = Math.max(
              slowClientResumeAfterEventId,
              getRuntimeLastEventId(runtime),
              normalizeStreamEventId(message.stream_event_id) || 0
            );
          }
          const mutationBaseline = captureRealtimeWorkflowMutationBaseline(
            message,
            sessionMessagesRef
          );
          if (perfEnabled) {
            const start = performance.now();
            processor.handleEvent(normalizedEventType || eventType, dataText);
            chatPerf.recordDuration('chat_resume_event_handle', performance.now() - start, {
              eventType: normalizedEventType || eventType,
              sessionId
            });
          } else {
            processor.handleEvent(normalizedEventType || eventType, dataText);
          }
          logRealtimeWorkflowMutation({
            phase: 'resume',
            sessionId,
            eventType: normalizedEventType || eventType,
            eventId,
            roundNumber: message.stream_round,
            userRoundNumber: payload?.user_round ?? approvalPayload?.user_round,
            message,
            messages: sessionMessagesRef,
            before: mutationBaseline
          });
        };
        const requestId = resumeRequestId || buildWsRequestId();
        resumeRequestId = requestId;
        if (runtime) {
          runtime.resumeRequestId = requestId;
        }
        await chatWsClient.request({
          requestId,
          sessionId,
          message: {
            type: resumeAfterEventId > 0 ? 'resume' : 'watch',
            request_id: requestId,
            session_id: sessionId,
            payload: {
              after_event_id: resumeAfterEventId
            }
          },
          onEvent,
          signal: runtime?.resumeController?.signal,
          closeOnFinal: resumeAfterEventId > 0
        });
      } catch (error) {
        if (error?.name === 'AbortError') {
          aborted = true;
        } else {
          const transient =
            !finalSeen &&
            !errorSeen &&
            (error?.phase === 'connect' ||
              error?.phase === 'stream' ||
              error?.phase === 'slow_client' ||
              error?.name === 'TypeError');
          if (!transient) {
            const detail = error?.message || t('chat.workflow.resumeFailedDetail');
            errorSeen = true;
            message.workflowItems.push(
              buildWorkflowItem(
                t('chat.workflow.resumeFailed'),
                detail,
                'failed'
              )
            );
            if (!message.content) {
              message.content = detail;
            }
          } else if (perfEnabled) {
            chatPerf.count('chat_resume_interrupted', 1, { sessionId });
          }
        }
      } finally {
        const finishedRequestId = runtime?.resumeRequestId || '';
        const terminalSeen = finalSeen || errorSeen;
        let keepStreaming = !aborted && !terminalSeen;
        if (keepStreaming && runtime && isTerminalRuntimeStatus(runtime.threadStatus)) {
          keepStreaming = false;
        }
        message.workflowStreaming = keepStreaming;
        if (!aborted) {
          message.stream_incomplete = keepStreaming;
        }
        if (runtime) {
          clearRuntimeResumeStreamState(runtime);
          refreshRuntimeStreamLifecycle(runtime);
          if (!keepStreaming) {
            clearSlowClientResume(runtime);
          }
        }
        setSessionLoading(this, sessionId, keepStreaming);
        processor.finalize();
        touchSessionUpdatedAt(this, sessionId, Date.now());
        this.clearPendingApprovals({ requestId: finishedRequestId, sessionId });
        notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
        if (perfEnabled) {
          chatPerf.recordDuration('chat_resume_total', performance.now() - perfStreamStart, {
            sessionId,
            terminalSeen,
            aborted
          });
        }
        if (
          shouldRestartWatchAfterInteractiveStream({
            activeSessionId: this.activeSessionId,
            targetSessionId: sessionId,
            pageUnloading: chatPageLifecycle.pageUnloading
          })
        ) {
          if (keepStreaming && slowClientResumeAfterEventId > 0) {
            scheduleSlowClientResume(this, sessionId, message, slowClientResumeAfterEventId);
          }
          startSessionWatcher(this, sessionId);
        }
      }
    }
};
