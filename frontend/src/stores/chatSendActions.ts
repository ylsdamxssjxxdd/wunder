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

import { buildWorkflowItem, normalizeInquiryPanelState, safeJsonParse, syncDemoChatCache } from './chatDemoPanels';
import { applyGoalStreamEvent, applyMainSession, persistAgentSession } from './chatPersist';
import { abortWatchStream, clearDraftSessionBootstrapMarkers, clearDraftSessionBootstrapMessages, clearRuntimeSendStreamState, clearSlowClientResume, markAssistantMessageRequestFailed, markRuntimeSendStreamActivity, markRuntimeSendStreamStarted, resolveMaxStreamRound, resolveStreamFlushMsForMessages, setSessionLoading } from './chatRuntimeControls';
import { applySessionRuntimeEvent, cacheSessionMessages, captureRealtimeWorkflowMutationBaseline, clearSessionEventsSnapshot, ensureRuntime, handleThreadControlWorkflowEvent, logRealtimeWorkflowMutation, notifySessionSnapshot, refreshRuntimeStreamLifecycle, resolveSessionContextTokens, syncSessionContextTokens, touchSessionUpdatedAt } from './chatRuntimeState';
import { settleTerminalAssistantArtifacts as settleTerminalAssistantArtifactsBase } from './chatTerminalArtifacts';
import { chatPageLifecycle } from './chatSharedState';
import { buildMessage, resolveTimestampMs } from './chatStats';
import { assignStreamEventId, getRuntimeLastEventId, normalizeApprovalMode, normalizeStreamEventId, updateRuntimeLastEventId } from './chatStreamIds';
import { SendMessageOptions } from './chatTypes';
import { abortResumeStream, abortSendStream, buildWsRequestId, chatWsClient, scheduleSlowClientResume, startSessionWatcher } from './chatWatcher';
import { buildDetail, buildSessionTitle, getSessionWorkflowState, handleApprovalEvent, isTerminalLlmOutputPayload, isTerminalRuntimeStatus, isTerminalStreamEventType, resolveNormalizedStreamEventType, shouldAutoTitle, shouldTreatRuntimeEventAsTerminal } from './chatWorkflowHydration';
import { createWorkflowProcessor } from './chatWorkflowProcessor';

export const chatSendActions = {
    async sendMessage(content: string, options: SendMessageOptions = {}) {
      const initialSessionId = this.activeSessionId;
      abortResumeStream(initialSessionId);
      abortSendStream(initialSessionId);
      abortWatchStream(initialSessionId);
      clearSessionEventsSnapshot(initialSessionId);
      this.messages.forEach((message) => {
        if (message && typeof message === 'object') {
          (message as Record<string, unknown>).resume_available = false;
        }
      });
      clearSupersededPendingAssistantMessages(this.messages);
      const supersededPendingAssistant = findPendingAssistantMessage(this.messages);
      if (stopPendingAssistantMessage(supersededPendingAssistant)) {
        const panel = normalizeInquiryPanelState(supersededPendingAssistant?.questionPanel);
        if (panel?.status === 'pending') {
          supersededPendingAssistant.questionPanel = { ...panel, status: 'dismissed' };
        }
      }
      clearTrailingPendingAssistantMessages(this.messages);
      if (!this.activeSessionId) {
        clearDraftSessionBootstrapMessages(this.messages);
      }
      const perfEnabled = chatPerf.enabled();
      const perfStreamStart = perfEnabled ? performance.now() : 0;
      const initialRuntime = ensureRuntime(initialSessionId);
      if (initialRuntime) {
        initialRuntime.stopRequested = false;
      }
      const attachments = Array.isArray(options.attachments) ? options.attachments : [];
      const inputOverflow = resolveChatRequestTextInputOverflow(content, attachments, ({ actualChars, maxChars }) =>
        t('chat.error.userInputTooLong', { actualChars, maxChars })
      );
      if (inputOverflow) {
        throw buildChatRequestTextInputOverflowError(inputOverflow);
      }
      const bootstrappingDraftSession = !this.activeSessionId;
      const userMessage = buildMessage('user', content) as ReturnType<typeof buildMessage> & {
        draft_session_bootstrap?: boolean;
        attachments?: Array<{
          type?: unknown;
          name?: unknown;
          content?: unknown;
          mime_type?: unknown;
          public_path?: unknown;
        }>;
      };
      if (bootstrappingDraftSession) {
        userMessage.draft_session_bootstrap = true;
      }
      if (attachments.length > 0) {
        userMessage.attachments = attachments.map((item) => ({
          type: (item as { type?: unknown })?.type,
          name: (item as { name?: unknown })?.name,
          content: (item as { content?: unknown })?.content,
          mime_type: (item as { mime_type?: unknown })?.mime_type,
          public_path: (item as { public_path?: unknown })?.public_path
        }));
      }
      const requestStartMs = resolveTimestampMs(userMessage.created_at) ?? Date.now();
      const suppressQueuedNotice = options.suppressQueuedNotice === true;
      const nextLocalStreamRound = (resolveMaxStreamRound(this.messages) || 0) + 1;
      const assistantMessageRaw = {
        ...buildMessage('assistant', ''),
        ...(bootstrappingDraftSession ? { draft_session_bootstrap: true } : {}),
        workflowItems: [],
        workflowStreaming: true,
        stream_incomplete: true,
        waiting_updated_at_ms: requestStartMs,
        waiting_first_output_at_ms: null,
        waiting_phase_first_output_at_ms: null,
        stream_event_id: 0,
        stream_round: nextLocalStreamRound > 0 ? nextLocalStreamRound : null
      };
      if (assistantMessageRaw.stats) {
        assistantMessageRaw.stats.interaction_start_ms = requestStartMs;
      }
      if (bootstrappingDraftSession) {
        this.messages.push(userMessage);
        this.messages.push(assistantMessageRaw);
        this.scheduleSnapshot(true);
      }
      try {
        if (!this.activeSessionId) {
          const payload = this.draftAgentId ? { agent_id: this.draftAgentId } : {};
          const session = await this.createSession(payload, {
            preserveCurrentMessages: bootstrappingDraftSession
          });
          if (Array.isArray(this.draftToolOverrides)) {
            try {
              await this.updateSessionTools(session.id, this.draftToolOverrides);
            } catch (error) {
              // ignore draft tool override failures
            }
          }
          this.draftToolOverrides = null;
        }
      } catch (error) {
        if (bootstrappingDraftSession) {
          markAssistantMessageRequestFailed(
            assistantMessageRaw,
            error?.message || t('chat.workflow.requestFailedDetail')
          );
          this.dismissPendingInquiryPanel();
          this.scheduleSnapshot(true);
        }
        throw error;
      }
      const sessionId = this.activeSessionId;
      const runtime = ensureRuntime(sessionId);
      const previousSessionContextTokens = resolveSessionContextTokens(this, sessionId);
      if (!bootstrappingDraftSession) {
        this.messages.push(userMessage);
      }
      const sessionMessagesRef = this.messages;
      if (!bootstrappingDraftSession) {
        sessionMessagesRef.push(assistantMessageRaw);
      }
      const assistantMessage = assistantMessageRaw;
      chatDebugLog('messenger.send', 'store-placeholder-appended', {
        sessionId,
        bootstrappingDraftSession,
        messageCount: Array.isArray(sessionMessagesRef) ? sessionMessagesRef.length : 0,
        assistantPending: true,
        assistantStreamRound: assistantMessage.stream_round ?? null
      });
      clearDraftSessionBootstrapMarkers(sessionMessagesRef);
      cacheSessionMessages(sessionId, sessionMessagesRef);
      touchSessionUpdatedAt(this, sessionId, userMessage.created_at);

      const activeSession = this.sessions.find((item) => item.id === sessionId);
      if (activeSession) {
        this.sessions = applyMainSession(this.sessions, activeSession.agent_id, sessionId);
        persistAgentSession(activeSession.agent_id, sessionId);
        const hasExistingUserMessage = sessionMessagesRef.some(
          (message) => message !== userMessage && String(message?.role || '').trim() === 'user'
        );
        if (!hasExistingUserMessage && shouldAutoTitle(activeSession.title)) {
          const autoTitle = buildSessionTitle(content);
          if (autoTitle) {
            activeSession.title = autoTitle;
          }
        }
      }

      notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);

      setSessionLoading(this, sessionId, true);

      const workflowState = getSessionWorkflowState(sessionId);
      const processor = createWorkflowProcessor(
        assistantMessage,
        workflowState,
        (immediate = false) => notifySessionSnapshot(this, sessionId, sessionMessagesRef, immediate),
        {
          streamFlushMs: resolveStreamFlushMsForMessages(sessionMessagesRef),
          sessionId,
          initialContextTokens: previousSessionContextTokens,
          onThreadControl: (payload) => handleThreadControlWorkflowEvent(this, payload),
          onContextUsage: (contextTokens, contextTotalTokens) =>
            syncSessionContextTokens(this, sessionId, contextTokens, contextTotalTokens)
        }
      );
      let queued = false;
      let interruptedByStop = false;
      let recoveredByRealtime = false;
      let finalSeen = false;
      let errorSeen = false;
      let slowClientResumeAfterEventId = 0;
      let sendRequestId = '';

      try {
        if (runtime) {
          clearSlowClientResume(runtime);
          sendRequestId = buildWsRequestId();
          runtime.sendController = new AbortController();
          runtime.sendRequestId = sendRequestId;
          markRuntimeSendStreamStarted(runtime);
          refreshRuntimeStreamLifecycle(runtime);
        }
        const desktopToolCallMode = getDesktopToolCallModeForRequest();
        const approvalMode = normalizeApprovalMode(options.approvalMode ?? options.approval_mode);
        const debugPayloadEnabled = isChatDebugEnabled();
        const payload = {
          content,
          stream: true,
          ...(debugPayloadEnabled ? { debug_payload: true } : {}),
          ...(attachments.length > 0 ? { attachments } : {}),
          ...(desktopToolCallMode ? { tool_call_mode: desktopToolCallMode } : {}),
          ...(approvalMode ? { approval_mode: approvalMode } : {})
        };
        chatDebugLog('chat.llm.request', 'submit-start', {
          sessionId,
          transportHint: 'ws',
          debugPayloadEnabled,
          approvalMode: approvalMode || null,
          attachmentCount: attachments.length
        });
        const onEvent = (eventType, dataText, eventId) => {
          markRuntimeSendStreamActivity(runtime);
          const payload = safeJsonParse(dataText);
          const approvalPayload = payload?.data ?? payload;
          const normalizedEventType = resolveNormalizedStreamEventType(eventType, payload);
          if (applyGoalStreamEvent(this, sessionId, normalizedEventType, approvalPayload)) {
            return;
          }
          handleApprovalEvent(
            this,
            normalizedEventType || eventType,
            approvalPayload,
            runtime?.sendRequestId || '',
            sessionId
          );
          if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
            chatDebugLog('chat.store.terminal-debug', 'send-runtime-event', {
              sessionId,
              eventType: normalizedEventType,
              eventId: normalizeStreamEventId(eventId),
              payloadStatus: String(
                approvalPayload?.thread_status ?? approvalPayload?.status ?? ''
              ).trim().toLowerCase(),
              loadingBySession: Boolean(this.loadingBySession?.[sessionId]),
              runtimeBefore: buildRuntimeDebugSnapshot(runtime),
              latestAssistantFlags: {
                workflowStreaming: Boolean(assistantMessage.workflowStreaming),
                reasoningStreaming: Boolean(assistantMessage.reasoningStreaming),
                streamIncomplete: Boolean(assistantMessage.stream_incomplete),
                resumeAvailable: Boolean(assistantMessage.resume_available),
                slowClient: Boolean(assistantMessage.slow_client)
              }
            });
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
            updateRuntimeLastEventId(runtime, eventId);
            applySessionRuntimeEvent(this, sessionId, approvalPayload, normalizedEventType);
            return;
          }
          if (perfEnabled) {
            chatPerf.count('chat_stream_event', 1, { eventType: normalizedEventType || eventType, sessionId });
          }
          if (normalizedEventType === 'heartbeat' || normalizedEventType === 'ping') {
            return;
          }
          const queuedFlag =
            normalizedEventType === 'queued' || payload?.queued === true || payload?.data?.queued === true;
          if (queuedFlag) {
            if (!queued) {
              queued = true;
              if (!suppressQueuedNotice) {
                assistantMessage.workflowItems.push(
                  buildWorkflowItem(t('chat.workflow.queued'), buildDetail(payload?.data ?? payload), 'pending', {
                    eventType: 'queued'
                  })
                );
                notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
              }
            }
            assistantMessage.stream_incomplete = true;
            assistantMessage.workflowStreaming = true;
            return;
          }
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
              normalizeStreamEventId(assistantMessage.stream_event_id) || 0
            );
          }
          const normalizedEventId = normalizeStreamEventId(eventId);
          if (normalizedEventId !== null) {
            const currentEventId = Math.max(
              normalizeStreamEventId(assistantMessage.stream_event_id) || 0,
              getRuntimeLastEventId(runtime)
            );
            if (normalizedEventId <= currentEventId) {
              return;
            }
          }
          assignStreamEventId(assistantMessage, eventId);
          updateRuntimeLastEventId(runtime, eventId);
          const mutationBaseline = captureRealtimeWorkflowMutationBaseline(
            assistantMessage,
            sessionMessagesRef
          );
          if (perfEnabled) {
            const start = performance.now();
            processor.handleEvent(normalizedEventType || eventType, dataText);
            chatPerf.recordDuration('chat_stream_event_handle', performance.now() - start, {
              eventType: normalizedEventType || eventType,
              sessionId
            });
          } else {
            processor.handleEvent(normalizedEventType || eventType, dataText);
          }
          logRealtimeWorkflowMutation({
            phase: 'send',
            sessionId,
            eventType: normalizedEventType || eventType,
            eventId,
            roundNumber: assistantMessage.stream_round,
            userRoundNumber: payload?.user_round ?? approvalPayload?.user_round,
            message: assistantMessage,
            messages: sessionMessagesRef,
            before: mutationBaseline
          });
        };
        const requestId = sendRequestId || buildWsRequestId();
        sendRequestId = requestId;
        if (runtime) {
          runtime.sendRequestId = requestId;
        }
        await chatWsClient.request({
          requestId,
          sessionId,
          message: {
            type: 'start',
            request_id: requestId,
            session_id: sessionId,
            payload
          },
          onEvent,
          signal: runtime?.sendController?.signal,
          closeOnFinal: true,
          resolveOnQueued: true,
          cancelOnAbort: false
        });
      } catch (error) {
        const abortReason = String(runtime?.sendAbortReason || '').trim();
        if (error?.name === 'AbortError' && abortReason === 'local_recovery') {
          recoveredByRealtime = true;
        } else if (error?.name === 'AbortError' || runtime?.stopRequested || chatPageLifecycle.pageUnloading) {
          interruptedByStop = true;
          assistantMessage.reasoningStreaming = false;
          if (!chatPageLifecycle.pageUnloading) {
            assistantMessage.workflowItems.push(
              buildWorkflowItem(
                t('chat.workflow.aborted'),
                t('chat.workflow.abortedDetail'),
                'failed'
              )
            );
            if (!assistantMessage.content) {
              assistantMessage.content = t('chat.workflow.aborted');
            }
          }
        } else {
          const transient =
            !finalSeen &&
            !errorSeen &&
            (error?.phase === 'connect' ||
              error?.phase === 'stream' ||
              error?.phase === 'slow_client' ||
              error?.name === 'TypeError');
          if (!transient) {
            const detail = error?.message || t('chat.workflow.requestFailedDetail');
            errorSeen = true;
            const normalizedDetail = String(detail || '').trim().toLowerCase();
            const looksLikeOverflow = [
              'context_window_exceeded',
              'context length exceeded',
              'context window',
              'input exceeds the context window',
              'exceeds the model',
              'prompt is too long',
              '上下文',
              '超限',
              '过长'
            ].some((token) => normalizedDetail.includes(token));
            if (looksLikeOverflow) {
              for (let cursor = assistantMessage.workflowItems.length - 1; cursor >= 0; cursor -= 1) {
                const item = assistantMessage.workflowItems[cursor];
                if (item?.eventType !== 'compaction_progress') continue;
                if (item?.status !== 'loading' && item?.status !== 'pending') continue;
                const existingDetail = safeJsonParse(item.detail);
                item.status = 'failed';
                item.detail = buildDetail({
                  ...(existingDetail && typeof existingDetail === 'object' ? existingDetail : {}),
                  status: 'failed',
                  stage: 'context_overflow_recovery',
                  error_code: 'CONTEXT_WINDOW_EXCEEDED',
                  error_message: String(detail || '')
                });
                break;
              }
            }
            assistantMessage.workflowItems.push(
              buildWorkflowItem(
                t('chat.workflow.requestFailed'),
                detail,
                'failed',
                { eventType: 'request_failed' }
              )
            );
            if (!assistantMessage.content) {
              assistantMessage.content = detail;
            }
          } else if (perfEnabled) {
            chatPerf.count('chat_stream_interrupted', 1, { sessionId });
          }
        }
        this.dismissPendingInquiryPanel();
      } finally {
        const stopped = interruptedByStop || Boolean(runtime?.stopRequested);
        const terminalSeen = finalSeen || errorSeen;
        let keepStreaming = recoveredByRealtime || (!stopped && !terminalSeen);
        if (keepStreaming && runtime && isTerminalRuntimeStatus(runtime.threadStatus)) {
          keepStreaming = false;
        }
        const finishedRequestId = runtime?.sendRequestId || '';
        assistantMessage.workflowStreaming = keepStreaming;
        assistantMessage.reasoningStreaming = false;
        assistantMessage.stream_incomplete = keepStreaming;
        if (runtime) {
          clearRuntimeSendStreamState(runtime);
          runtime.sendAbortReason = '';
          runtime.stopRequested = false;
          refreshRuntimeStreamLifecycle(runtime);
          if (!keepStreaming) {
            clearSlowClientResume(runtime);
          }
        }
        if (!keepStreaming) {
          settleTerminalAssistantArtifactsBase(sessionMessagesRef, {
            failed: errorSeen || stopped
          });
        }
        setSessionLoading(this, sessionId, keepStreaming);
        processor.finalize();
        touchSessionUpdatedAt(this, sessionId, Date.now());
        this.clearPendingApprovals({ requestId: finishedRequestId, sessionId });
        syncDemoChatCache({
          sessions: this.sessions,
          sessionId,
          messages: sessionMessagesRef
        });
        notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
        if (perfEnabled) {
          chatPerf.recordDuration('chat_stream_total', performance.now() - perfStreamStart, {
            sessionId,
            terminalSeen,
            stopped,
            queued
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
            scheduleSlowClientResume(this, sessionId, assistantMessage, slowClientResumeAfterEventId);
          }
          startSessionWatcher(this, sessionId);
        }
      }
    },
};
