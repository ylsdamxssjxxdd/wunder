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
import { buildMessageIdentityDebugList, buildMessageIdentityDebugSnapshot } from '@/utils/chatMessageDebug';
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
  selectRuntimeLastAppliedEventId,
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

import { buildWorkflowItem, normalizeInquiryPanelState, safeJsonParse, syncDemoChatCache } from './chatDemoPanels';
import { applyGoalStreamEvent, applyMainSession, persistAgentSession } from './chatPersist';
import { abortWatchStream, clearDraftSessionBootstrapMarkers, clearDraftSessionBootstrapMessages, clearRuntimeSendStreamState, clearSlowClientResume, markAssistantMessageRequestFailed, markRuntimeSendStreamActivity, markRuntimeSendStreamStarted, resolveMaxStreamRound, setSessionLoading } from './chatRuntimeControls';
import { applyCanonicalClientMessageSubmittedRuntimeEvent, applyCanonicalStreamRuntimeEvent, applyLocalAssistantTurnTerminalRuntimeEvent, applySessionRuntimeEvent, buildRuntimeDebugSnapshot, cacheSessionMessages, clearSessionEventsSnapshot, ensureRuntime, notifySessionSnapshot, refreshRuntimeStreamLifecycle, touchSessionUpdatedAt } from './chatRuntimeState';
import { settleTerminalAssistantArtifacts as settleTerminalAssistantArtifactsBase } from './chatTerminalArtifacts';
import { chatPageLifecycle } from './chatSharedState';
import { buildMessage, resolveTimestampMs } from './chatStats';
import { getRuntimeLastEventId, normalizeApprovalMode, normalizeStreamEventId, updateRuntimeLastEventId } from './chatStreamIds';
import { SendMessageOptions } from './chatTypes';
import { abortResumeStream, abortSendStream, buildWsRequestId, chatWsClient, scheduleSlowClientResume, startSessionWatcher } from './chatWatcher';
import { buildDetail, buildSessionTitle, handleApprovalEvent, isTerminalLlmOutputPayload, isTerminalStreamEventType, resolveNormalizedStreamEventType, shouldAutoTitle, shouldTreatRuntimeEventAsTerminal } from './chatWorkflowHydration';
import { shouldUseProjectionOnlyInteractiveStreamEvent } from './chatProjectionOnlyEvents';

const RUNTIME_PENDING_GAP_RECOVERY_DELAY_MS = 150;

const resolveMaxProjectionUserRound = (projection: ChatRuntimeProjection | null | undefined, sessionId: unknown): number => {
  const key = String(sessionId ?? '').trim();
  if (!key || !projection?.sessions?.[key]) return 0;
  const session = projection.sessions[key];
  const maxExplicitRound = session.userTurns.reduce((maxRound, turnId) => {
    const match = /^user-turn:[^:]+:round:(\d+)$/.exec(String(turnId || '').trim());
    if (!match) return maxRound;
    const round = Number.parseInt(match[1], 10);
    return Number.isFinite(round) && round > maxRound ? round : maxRound;
  }, 0);
  return Math.max(maxExplicitRound, session.userTurns.length);
};

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
      const maxKnownStreamRound = Math.max(
        resolveMaxStreamRound(this.messages) || 0,
        resolveMaxProjectionUserRound(this.runtimeProjection, this.activeSessionId)
      );
      const nextLocalStreamRound = maxKnownStreamRound + 1;
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
      const clientMessageId = String(
        (userMessage as Record<string, unknown>).message_id ??
          (userMessage as Record<string, unknown>).messageId ??
          (userMessage as Record<string, unknown>).id ??
          `local-user:${sessionId}:${requestStartMs}`
      ).trim();
      const localUserTurnId = `user-turn:${sessionId}:round:${nextLocalStreamRound}`;
      const localModelTurnId = `model-turn:${sessionId}:user:${nextLocalStreamRound}:model:1`;
      Object.assign(userMessage as Record<string, unknown>, {
        message_id: clientMessageId,
        client_message_id: clientMessageId,
        user_turn_id: localUserTurnId,
        stream_round: nextLocalStreamRound > 0 ? nextLocalStreamRound : null
      });
      Object.assign(assistantMessageRaw as Record<string, unknown>, {
        user_turn_id: localUserTurnId,
        model_turn_id: localModelTurnId,
        // Assign a stable message_id derived from model_turn_id so the render
        // key never degrades to an index-based fallback. This prevents full
        // remounts (and streaming-state loss) when history is prepended or when
        // a user message is spliced in ahead of the pending assistant bubble.
        // The reducer reuses this id when the backend assistant_message_created
        // event arrives (matched via model_turn_id), so no duplicate is created.
        message_id: `local-assistant:${localModelTurnId}`,
        client_message_id: `local-assistant:${localModelTurnId}`
      });
      const sessionMessagesRef = this.messages;
      const assistantMessage = assistantMessageRaw;
      applyCanonicalClientMessageSubmittedRuntimeEvent(this, {
        sessionId,
        content,
        clientMessageId,
        createdAt: userMessage.created_at,
        userTurnId: localUserTurnId,
        modelTurnId: localModelTurnId,
        assistantMessageId: `local-assistant:${localModelTurnId}`,
        attachments: userMessage.attachments
      });
      chatDebugLog('messenger.send', 'store-placeholder-appended', {
        sessionId,
        bootstrappingDraftSession,
        messageCount: Array.isArray(sessionMessagesRef) ? sessionMessagesRef.length : 0,
        assistantPending: true,
        assistantStreamRound: assistantMessage.stream_round ?? null,
        identity: {
          user: buildMessageIdentityDebugSnapshot(userMessage, sessionMessagesRef.indexOf(userMessage)),
          assistant: buildMessageIdentityDebugSnapshot(
            assistantMessage,
            sessionMessagesRef.indexOf(assistantMessage)
          )
        },
        messages: buildMessageIdentityDebugList(sessionMessagesRef)
      });
      if (bootstrappingDraftSession) {
        clearDraftSessionBootstrapMarkers(sessionMessagesRef);
      }
      cacheSessionMessages(sessionId, sessionMessagesRef);
      touchSessionUpdatedAt(this, sessionId, userMessage.created_at);

      const activeSession = this.sessions.find((item) => item.id === sessionId);
      if (activeSession) {
        this.sessions = applyMainSession(this.sessions, activeSession.agent_id, sessionId);
        persistAgentSession(activeSession.agent_id, sessionId);
        const hasExistingLegacyUserMessage = sessionMessagesRef.some(
          (message) => message !== userMessage && String(message?.role || '').trim() === 'user'
        );
        const hasExistingProjectedUserMessage = selectVisibleMessageProjections(this.runtimeProjection, sessionId).some(
          (message) => message.id !== clientMessageId && message.role === 'user'
        );
        if (!hasExistingLegacyUserMessage && !hasExistingProjectedUserMessage && shouldAutoTitle(activeSession.title)) {
          const autoTitle = buildSessionTitle(content);
          if (autoTitle) {
            activeSession.title = autoTitle;
          }
        }
      }

      if (bootstrappingDraftSession) {
        notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
      }

      setSessionLoading(this, sessionId, true);

      let queued = false;
      let interruptedByStop = false;
      let recoveredByRealtime = false;
      let finalSeen = false;
      let errorSeen = false;
      let slowClientResumeAfterEventId = 0;
      let sendRequestId = '';
      const hasProjectionSession = Boolean(this.runtimeProjection?.sessions?.[sessionId]);
      const syncRuntimeLastAppliedEventId = () => {
        const appliedEventId = selectRuntimeLastAppliedEventId(this.runtimeProjection, sessionId);
        updateRuntimeLastEventId(
          runtime,
          appliedEventId || (hasProjectionSession ? 0 : getRuntimeLastEventId(runtime))
        );
        return appliedEventId;
      };

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
          client_message_id: clientMessageId,
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
          const terminalLlmOutput =
            normalizedEventType === 'llm_output' &&
            isTerminalLlmOutputPayload(payload, approvalPayload);
          const projectionOnlyInteractiveEvent = shouldUseProjectionOnlyInteractiveStreamEvent(
            normalizedEventType,
            { terminalLlmOutput }
          );
          if (applyGoalStreamEvent(this, sessionId, normalizedEventType, approvalPayload)) {
            return;
          }
          applyCanonicalStreamRuntimeEvent(
            this,
            sessionId,
            normalizedEventType || eventType,
            payload,
            eventId,
            {
              requestId: runtime?.sendRequestId || sendRequestId || requestId,
              phase: 'send',
              sideEffects: projectionOnlyInteractiveEvent,
              onSyncRequired: (reason) => {
                const run = () => {
                  void this.ensureActiveSessionRealtime({
                    sessionId,
                    reason: String(reason || '') === 'event_seq_gap'
                      ? 'send_event_seq_gap'
                      : 'send_pending_event_seq_gap',
                    forceHydrate: true
                  }).catch(() => {});
                };
                if (String(reason || '') === 'event_seq_gap') {
                  run();
                  return;
                }
                globalThis.setTimeout(run, RUNTIME_PENDING_GAP_RECOVERY_DELAY_MS);
              }
            }
          );
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
              },
              latestAssistantIdentity: buildMessageIdentityDebugSnapshot(
                assistantMessage,
                sessionMessagesRef.indexOf(assistantMessage)
              ),
              messages: buildMessageIdentityDebugList(sessionMessagesRef)
            });
            if (shouldTreatRuntimeEventAsTerminal(normalizedEventType, approvalPayload)) {
              const runtimeStatus = normalizeThreadRuntimeStatus(
                approvalPayload?.thread_status ?? approvalPayload?.status
              );
              if (runtimeStatus === 'system_error') {
                errorSeen = true;
              }
            }
            syncRuntimeLastAppliedEventId();
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
            normalizedEventType === 'queued' ||
            normalizedEventType === 'queue_enter' ||
            normalizedEventType === 'queue_update' ||
            payload?.queued === true ||
            payload?.data?.queued === true;
          if (queuedFlag) {
            if (!queued) {
              queued = true;
            }
            syncRuntimeLastAppliedEventId();
            return;
          }
          if (isTerminalStreamEventType(normalizedEventType)) {
            if (normalizedEventType === 'error' || normalizedEventType === 'queue_fail') {
              errorSeen = true;
            } else {
              finalSeen = true;
            }
          } else if (terminalLlmOutput) {
            finalSeen = true;
          } else if (
            normalizedEventType === 'slow_client' &&
            String(payload?.reason ?? payload?.data?.reason ?? '').trim() === 'queue_full_resume_required'
          ) {
            const appliedEventId = selectRuntimeLastAppliedEventId(this.runtimeProjection, sessionId);
            slowClientResumeAfterEventId = Math.max(
              slowClientResumeAfterEventId,
              appliedEventId,
              hasProjectionSession ? 0 : getRuntimeLastEventId(runtime),
              normalizeStreamEventId(assistantMessage.stream_event_id) || 0
            );
          }
          syncRuntimeLastAppliedEventId();
          return;
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
          resolveOnQueued: false,
          keepPendingAfterQueuedAck: false,
          cancelOnAbort: false
        });
      } catch (error) {
        const abortReason = String(runtime?.sendAbortReason || '').trim();
        if (error?.name === 'AbortError' && abortReason === 'local_recovery') {
          recoveredByRealtime = true;
        } else if (error?.name === 'AbortError' || runtime?.stopRequested || chatPageLifecycle.pageUnloading) {
          interruptedByStop = true;
          if (bootstrappingDraftSession) {
            assistantMessage.reasoningStreaming = false;
          }
          if (!chatPageLifecycle.pageUnloading) {
            applyLocalAssistantTurnTerminalRuntimeEvent(this, {
              sessionId,
              terminal: 'cancelled',
              content: t('chat.workflow.aborted'),
              reason: 'user_stop',
              requestId: sendRequestId,
              userTurnId: localUserTurnId,
              modelTurnId: localModelTurnId,
              assistantMessageId: `local-assistant:${localModelTurnId}`
            });
            if (bootstrappingDraftSession) {
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
            applyLocalAssistantTurnTerminalRuntimeEvent(this, {
              sessionId,
              terminal: 'failed',
              content: detail,
              reason: 'request_failed',
              requestId: sendRequestId,
              userTurnId: localUserTurnId,
              modelTurnId: localModelTurnId,
              assistantMessageId: `local-assistant:${localModelTurnId}`
            });
            const normalizedDetail = String(detail || '').trim().toLowerCase();
            const looksLikeOverflow = [
              'context_window_exceeded',
              'context length exceeded',
              'context window',
              'input exceeds the context window',
              'exceeds the model',
              'prompt is too long',
              'context',
              '瓒呴檺',
              '杩囬暱'
            ].some((token) => normalizedDetail.includes(token));
            if (looksLikeOverflow) {
              if (bootstrappingDraftSession) {
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
            }
            if (bootstrappingDraftSession) {
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
            }
          } else if (perfEnabled) {
            chatPerf.count('chat_stream_interrupted', 1, { sessionId });
          }
        }
        this.dismissPendingInquiryPanel();
      } finally {
        const currentSendRequestId = String(runtime?.sendRequestId || '').trim();
        const ownsCurrentSendState = Boolean(runtime) && currentSendRequestId === sendRequestId;
        const sendControllerAlreadyCleared =
          Boolean(runtime) && !runtime.sendController && currentSendRequestId !== sendRequestId;
        const sendStateAlreadySettledForThisRequest =
          Boolean(runtime) && !runtime.sendController && !currentSendRequestId;
        const stopped =
          interruptedByStop ||
          Boolean(runtime?.stopRequested) ||
          sendControllerAlreadyCleared;
        const terminalSeen = finalSeen || errorSeen;
        let keepStreaming = recoveredByRealtime || (!stopped && !terminalSeen);
        const finishedRequestId = ownsCurrentSendState ? currentSendRequestId : sendRequestId;
        if (bootstrappingDraftSession) {
          assistantMessage.workflowStreaming = keepStreaming;
          assistantMessage.reasoningStreaming = false;
          assistantMessage.stream_incomplete = keepStreaming;
        }
        if (runtime) {
          const clearedOwnSendState = clearRuntimeSendStreamState(runtime, {
            requestId: sendRequestId
          });
          const canSettleStopMarker = clearedOwnSendState || sendStateAlreadySettledForThisRequest;
          if (canSettleStopMarker) {
            runtime.sendAbortReason = '';
            runtime.stopRequested = false;
          }
          refreshRuntimeStreamLifecycle(runtime);
          if (canSettleStopMarker && !keepStreaming) {
            clearSlowClientResume(runtime);
          }
        }
        const canApplyGlobalSendSettlement = ownsCurrentSendState || sendStateAlreadySettledForThisRequest;
        if (bootstrappingDraftSession && canApplyGlobalSendSettlement && !keepStreaming) {
          settleTerminalAssistantArtifactsBase(sessionMessagesRef, {
            failed: errorSeen || stopped
          });
        }
        if (canApplyGlobalSendSettlement) {
          setSessionLoading(this, sessionId, keepStreaming);
        }
        touchSessionUpdatedAt(this, sessionId, Date.now());
        if (canApplyGlobalSendSettlement) {
          this.clearPendingApprovals({ requestId: finishedRequestId || sendRequestId, sessionId });
        }
        syncDemoChatCache({
          sessions: this.sessions,
          sessionId,
          messages: sessionMessagesRef
        });
        if (bootstrappingDraftSession) {
          notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
        }
        chatDebugLog('messenger.send', 'stream-finish', {
          sessionId,
          requestId: finishedRequestId || null,
          stopped,
          terminalSeen,
          queued,
          keepStreaming,
          runtime: runtime ? buildRuntimeDebugSnapshot(runtime) : null,
          latestAssistant: buildMessageIdentityDebugSnapshot(assistantMessage, sessionMessagesRef.indexOf(assistantMessage)),
          messages: buildMessageIdentityDebugList(sessionMessagesRef)
        });
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
            scheduleSlowClientResume(this, sessionId, null, slowClientResumeAfterEventId);
          }
          startSessionWatcher(this, sessionId);
        }
      }
    },
};
