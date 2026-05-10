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

import { applyPlanUpdate, buildToolIdentityMeta, buildWorkflowItem, hasPlanSteps, isQuestionPanelToolName, normalizeInquiryPanelPayload, normalizeInquiryPanelState, normalizePlanPayload, safeJsonParse, shouldAutoShowPlan, tailText } from './chatDemoPanels';
import { applyDesktopOverlayEvent } from './chatPersist';
import { STREAM_FLUSH_BASE_MS, resolveStreamFlushMs } from './chatRuntimeControls';
import { resolveSessionKey, sessionSubagentsCache } from './chatRuntimeState';
import { buildWorkflowModelRoundUsageMeta, buildWorkflowTimingMeta, buildWorkflowUsageMeta, clearAssistantRetryState, collectSubagentPayloads, collectWorkspacePathHints, combineWorkflowUsageMeta, ensureMessageStats, estimateStreamOutputTokens, hasWorkflowUsageConsumedTokens, markAssistantRetryState, markAssistantWaitingOutputVisible, mergeWorkflowUsageSnapshot, normalizeContextTokens, normalizeContextTotalTokens, normalizeDurationValue, normalizeMessageSubagents, normalizeQuotaSnapshot, normalizeSpeedValue, normalizeStatsCount, normalizeSubagentEventStatus, normalizeUsagePayload, parseOptionalCount, resetAssistantWaitingOutputPhase, resolveExplicitContextTokens, resolveInteractionDuration, resolveTimestampMs, resolveUsageConsumedTokensFromPayload, summarizeWorkflowUsageDebug, touchAssistantWaitingActivity, upsertMessageSubagent } from './chatStats';
import { normalizeFlag, normalizeStreamRound, parseSegmentedDelta, readDeltaSegments } from './chatStreamIds';
import { NormalizedUsagePayload, QuestionPanelApplyOptions, UsageStatsOptions, WorkflowProcessorOptions } from './chatTypes';
import { buildDetail, cloneCompactionDebugPayload, createThinkTagStreamParser, extractFinalAnswerFromToolCalls, isFailedResult, normalizeAssistantOutput, normalizeReasoningText, normalizeSessionWorkflowState, pickString, pickText, resolveAssistantReasoning, resolveEventType, resolveToolCategory, toOptionalInt, updateWorkflowItem } from './chatWorkflowHydration';

export const handleWorkflowProcessorEvent = (ctx: any, eventName, raw) => {
    const payload = safeJsonParse(raw);
    const data = payload?.data ?? payload;
    const eventType = resolveEventType(eventName, payload);
    ctx.applyInteractionTimestamp(payload?.timestamp ?? data?.timestamp);
    if (eventType === 'heartbeat' || eventType === 'ping') {
      return;
    }
    touchAssistantWaitingActivity(ctx.assistantMessage, payload?.timestamp ?? data?.timestamp);

    // 基于事件类型生成工作流条目并更新回复内容
      switch (eventType) {
      case 'queue_enter':
      case 'queue_update': {
        const detailPayload = data ?? payload;
        const existingQueueItem = [...ctx.assistantMessage.workflowItems]
          .reverse()
          .find(
            (item) =>
              ['queue_enter', 'queue_update'].includes(String(item?.eventType || item?.event || '').trim()) &&
              String(item?.status || '').trim().toLowerCase() !== 'completed'
          );
        if (existingQueueItem?.id) {
          updateWorkflowItem(ctx.assistantMessage.workflowItems, existingQueueItem.id, {
            title: t('chat.workflow.queued'),
            detail: buildDetail(detailPayload),
            status: 'pending',
            eventType: 'queue_enter'
          });
        } else {
          ctx.assistantMessage.workflowItems.push(
            buildWorkflowItem(
              t('chat.workflow.queued'),
              buildDetail(detailPayload),
              'pending',
              { eventType: 'queue_enter' }
            )
          );
        }
        break;
      }
      case 'queue_start': {
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.queueStart'),
            buildDetail(data ?? payload),
            'loading',
            { eventType: 'queue_start' }
          )
        );
        break;
      }
      case 'queue_finish': {
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.queueFinish'),
            buildDetail(data ?? payload),
            'completed',
            { eventType: 'queue_finish' }
          )
        );
        break;
      }
      case 'queue_fail': {
        const detailPayload = data ?? payload;
        const detailText = pickText(
          data?.error ?? payload?.error ?? data?.message ?? payload?.message,
          t('chat.workflow.requestFailedDetail')
        );
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.queueFail'),
            buildDetail(detailPayload),
            'failed',
            { eventType: 'queue_fail' }
          )
        );
        if (!ctx.assistantMessage.content) {
          ctx.assistantMessage.content = detailText;
        }
        break;
      }
      case 'desktop_controller_hint':
      case 'desktop_controller_hint_done':
      case 'desktop_monitor_countdown':
      case 'desktop_monitor_countdown_done': {
        applyDesktopOverlayEvent(eventType, data);
        return;
      }
      case 'progress': {
        const stage = data?.stage ?? payload?.stage;
        const normalizedStage = String(stage ?? '').trim().toLowerCase();
        let summary = data?.summary ?? payload?.summary;
        let detailSource = data;
        if (stage === 'llm_call') {
          const roundNumber = ctx.advanceModelRound();
          summary = `调用模型（第 ${roundNumber} 轮）`;
          ctx.lastRound = roundNumber;
          // 保持详情中的轮次与会话累计轮次一致
          if (data && typeof data === 'object') {
            detailSource = { ...data, round: roundNumber };
          } else {
            detailSource = { stage, summary: data?.summary ?? payload?.summary ?? '调用模型', round: roundNumber };
          }
        }
        if (stage === 'tool_failure_guard') {
          ctx.appendToolFailureGuardWorkflowItem(data ?? payload);
          break;
        }
        if (
          normalizedStage === 'compacting'
          || normalizedStage === 'context_overflow_recovery'
          || normalizedStage === 'context_guard'
        ) {
          ctx.markManualCompactionMarker(detailSource);
          if (ctx.finalizeWithNow) {
            ctx.assistantMessage.workflowStreaming = true;
            ctx.assistantMessage.stream_incomplete = true;
          }
          // Ignore delayed compaction progress events after terminal state to avoid re-opening "running" UI.
          if (ctx.compactionTerminalStatusHint) {
            break;
          }
          summary = resolveCompactionProgressTitle(stage, summary, t) ?? summary;
          const round = ctx.resolveRound(payload, data);
          const workflowRef = ctx.resolveActiveCompactionWorkflowRef(round, detailSource);
          ctx.ensureCompactionProgressItem(
            pickText(summary) || t('chat.workflow.progressUpdate'),
            buildDetail(detailSource),
            workflowRef
          );
          chatDebugLog('chat.compaction.event', 'progress', {
            sessionId: ctx.options.sessionId ?? null,
            round,
            workflowRef,
            terminalHint: ctx.compactionTerminalStatusHint,
            payload: cloneCompactionDebugPayload(data ?? payload ?? {}, {})
          });
          break;
        }
        const showStage = stage && !['received', 'llm_call'].includes(stage);
        const title = summary ? pickText(summary) : showStage ? `阶段：${stage}` : '进度更新';
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(title, buildDetail(detailSource))
        );
        break;
      }
      case 'llm_request': {
        resetAssistantWaitingOutputPhase(ctx.assistantMessage, payload?.timestamp ?? data?.timestamp);
        clearAssistantRetryState(ctx.assistantMessage);
        const requestPayload = data ?? payload ?? {};
        const requestPurpose = String(data?.purpose ?? payload?.purpose ?? '').trim().toLowerCase();
        if (!isCompactionSummaryEvent('llm_request', requestPayload)) {
          ctx.updateLiveContextUsageFromRequest(requestPayload);
        }
        chatDebugLog('chat.llm.request', 'event', {
          sessionId: ctx.options.sessionId ?? null,
          round: ctx.resolveRound(payload, data),
          purpose: requestPurpose,
          payloadOmitted: Boolean(requestPayload?.payload_omitted),
          request: requestPayload
        });
        if (requestPurpose === 'compaction_summary') {
          chatDebugLog('chat.compaction.event', 'llm-request-compaction-summary', {
            sessionId: ctx.options.sessionId ?? null,
            payload: cloneCompactionDebugPayload(requestPayload, {})
          });
          break;
        }
        const requestHasPayload = data && typeof data === 'object' && 'payload' in data;
        const requestHasSummary = data && typeof data === 'object' && 'payload_summary' in data;
        const requestTitle =
          requestHasSummary && !requestHasPayload
            ? t('chat.workflow.modelRequestSummary')
            : t('chat.workflow.modelRequest');
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(requestTitle, buildDetail(data), 'completed', { eventType: 'llm_request' })
        );
        break;
      }
      case 'llm_stream_retry': {
        const attempt = parseOptionalCount(data?.attempt ?? payload?.attempt);
        const maxAttempts = parseOptionalCount(data?.max_attempts ?? payload?.max_attempts);
        const retryReason = String(data?.retry_reason ?? payload?.retry_reason ?? '').trim();
        const retryError = String(data?.error ?? payload?.error ?? '').trim();
        const retryDelayRaw = Number(data?.delay_s ?? payload?.delay_s);
        const retryDelay = Number.isFinite(retryDelayRaw) && retryDelayRaw > 0 ? retryDelayRaw : null;
        markAssistantRetryState(ctx.assistantMessage, {
          attempt,
          maxAttempts,
          delayS: retryDelay,
          startedAtMs: payload?.timestamp ?? data?.timestamp,
          reason: retryReason,
          error: retryError
        });
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.modelRetry'),
            buildDetail(data ?? payload),
            'pending',
            {
              eventType: 'llm_stream_retry',
              attempt,
              maxAttempts,
              retryReason,
              error: retryError,
              delayS: retryDelay
            }
          )
        );
        ctx.notifySnapshot(true);
        break;
      }
      case 'knowledge_request': {
        const base = data?.knowledge_base ?? data?.knowledgeBase ?? '';
        const title = base ? `知识库请求体（${base}）` : '知识库请求体';
        ctx.assistantMessage.workflowItems.push(buildWorkflowItem(title, buildDetail(data)));
        break;
      }
      case 'command_session_delta': {
        break;
      }
      case 'command_session_start': {
        const commandSessionId = ctx.extractCommandSessionRef(payload, data);
        if (!commandSessionId) {
          break;
        }
        const detailSource =
          data && typeof data === 'object'
            ? data
            : payload && typeof payload === 'object'
              ? payload
              : {};
        ctx.syncCommandSessionSnapshot(detailSource);
        ctx.ensureCommandSessionCallItem(commandSessionId, detailSource, detailSource?.command ?? '');
        break;
      }
      case 'command_session_status':
      case 'command_session_exit':
      case 'command_session_summary': {
        const commandSessionId = ctx.extractCommandSessionRef(payload, data);
        if (!commandSessionId) {
          break;
        }
        const detailSource =
          data && typeof data === 'object'
            ? data
            : payload && typeof payload === 'object'
              ? payload
              : {};
        ctx.syncCommandSessionSnapshot(detailSource);
        const command = pickString(detailSource?.command);
        ctx.ensureCommandSessionCallItem(commandSessionId, detailSource, command);
        const outputKey = ctx.resolveToolOutputKey(
          ctx.executeCommandToolName,
          commandSessionId,
          commandSessionId
        );
        const outputBuffer = ctx.getToolOutputBuffer(outputKey);
        if (command && !outputBuffer.command) {
          outputBuffer.command = command;
        }
        if (eventType === 'command_session_summary') {
          ctx.mergeCommandSessionSummaryIntoBuffer(outputBuffer, detailSource);
        }
        const toolCategory = resolveToolCategory(ctx.executeCommandToolName, detailSource);
        const outputItemId = ctx.ensureToolOutputItem(
          ctx.executeCommandToolName,
          outputKey,
          toolCategory,
          commandSessionId,
          ctx.buildCommandSessionTitle(
            command,
            toOptionalInt(detailSource?.command_index, detailSource?.commandIndex)
          ),
          { commandSessionId }
        );
        if (outputItemId) {
          ctx.clearToolOutputFlush(outputKey);
          updateWorkflowItem(ctx.assistantMessage.workflowItems, outputItemId, {
            detail: ctx.buildToolOutputDetail(outputBuffer),
            status: ctx.resolveCommandSessionStatus(
              detailSource,
              eventType === 'command_session_summary' ? 'completed' : 'loading'
            )
          });
        }
        break;
      }
      case 'tool_call': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name ?? '未知工具';
        const toolCallId = ctx.extractToolCallRef(payload, data);
        const commandSessionId = ctx.extractCommandSessionRef(payload, data);
        const detailSource = data && typeof data === 'object' ? data : payload ?? data;
        const toolCategory = resolveToolCategory(toolName, data ?? payload);
        const toolCallRound = ctx.resolveRound(payload, data);
        const usageMeta = combineWorkflowUsageMeta(
          buildWorkflowModelRoundUsageMeta(
            toolCallRound !== null ? ctx.modelRoundUsagePayloadMap.get(toolCallRound) : null
          ),
          buildWorkflowUsageMeta(detailSource, data, payload)
        );
        const timingMeta = buildWorkflowTimingMeta(detailSource, data, payload);
        if (ctx.isExecuteCommandTool(toolName)) {
          if (commandSessionId) {
            ctx.syncCommandSessionSnapshot(detailSource);
            ctx.ensureCommandSessionCallItem(
              commandSessionId,
              detailSource,
              detailSource?.command ?? ''
            );
          }
          ctx.registerToolStats(toolName);
          if (ctx.lastRound !== null) {
            ctx.blockedRounds.add(ctx.lastRound);
          }
          if (commandSessionId) {
            break;
          }
        }
        const item = buildWorkflowItem(`调用工具：${toolName}`, buildDetail(detailSource), 'loading', {
          isTool: true,
          toolCategory,
          eventType: 'tool_call',
          toolName: String(toolName || ''),
          toolCallId: toolCallId || commandSessionId || undefined,
          commandSessionId: commandSessionId || undefined,
          ...buildToolIdentityMeta(data, payload, detailSource),
          modelRound: toolCallRound ?? undefined,
          ...usageMeta,
          ...timingMeta
        });
        ctx.assistantMessage.workflowItems.push(item);
        ctx.registerToolItem(toolName, item.id, toolCallId || commandSessionId);
        if (!ctx.isExecuteCommandTool(toolName)) {
          ctx.registerToolStats(toolName);
        }
        if (ctx.lastRound !== null) {
          // 工具调用后不再接收该轮后续增量，但保留当前已展示的内容/思考。
          ctx.blockedRounds.add(ctx.lastRound);
        }
        break;
      }
      case 'tool_output_delta': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name ?? '';
        const toolCallId = ctx.extractToolCallRef(payload, data);
        const commandSessionId = ctx.extractCommandSessionRef(payload, data);
        const delta = data?.delta ?? payload?.delta ?? '';
        if (!delta) {
          break;
        }
        if (ctx.perfEnabled) {
          chatPerf.count('chat_tool_output_delta', 1);
        }
        const streamName = String(data?.stream ?? payload?.stream ?? 'stdout').toLowerCase();
        const command = typeof data?.command === 'string' ? data.command : payload?.command;
        const resolvedToolName = commandSessionId ? ctx.executeCommandToolName : toolName;
        const toolCategory = resolveToolCategory(resolvedToolName, data ?? payload);
        if (commandSessionId) {
          ctx.syncCommandSessionDelta(commandSessionId, streamName, delta, {
            command,
            command_index: data?.command_index ?? payload?.command_index,
            commandIndex: data?.commandIndex ?? payload?.commandIndex
          });
          ctx.ensureCommandSessionCallItem(commandSessionId, data ?? payload ?? {}, command ?? '');
          const outputKey = ctx.resolveToolOutputKey(
            resolvedToolName,
            commandSessionId,
            commandSessionId
          );
          const itemId = ctx.ensureToolOutputItem(
            resolvedToolName,
            outputKey,
            toolCategory,
            commandSessionId,
            ctx.buildCommandSessionTitle(command, data?.command_index ?? payload?.command_index),
            {
              commandSessionId,
              ...buildToolIdentityMeta(data, payload)
            }
          );
          if (itemId) {
            updateWorkflowItem(ctx.assistantMessage.workflowItems, itemId, {
              status: 'loading'
            });
          }
          break;
        }
        const callId = ctx.peekToolItemId(toolName, toolCallId);
        const outputKey = ctx.resolveToolOutputKey(
          resolvedToolName,
          toolCallId || callId,
          null
        );
        const buffer = ctx.getToolOutputBuffer(outputKey);
        if (command && !buffer.command) {
          buffer.command = String(command);
        }
        if (streamName.includes('err')) {
          ctx.appendToolOutput(buffer, 'stderr', delta);
        } else {
          ctx.appendToolOutput(buffer, 'stdout', delta);
        }
        const itemId = ctx.ensureToolOutputItem(
          resolvedToolName,
          outputKey,
          toolCategory,
          toolCallId,
          null,
          buildToolIdentityMeta(data, payload)
        );
        if (itemId) {
          ctx.scheduleToolOutputFlush(outputKey, itemId);
          updateWorkflowItem(ctx.assistantMessage.workflowItems, itemId, {
            ...buildToolIdentityMeta(data, payload)
          });
        }
        break;
      }
      case 'tool_result': {
        resetAssistantWaitingOutputPhase(ctx.assistantMessage, payload?.timestamp ?? data?.timestamp);
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name;
        const toolCallId = ctx.extractToolCallRef(payload, data);
        const result = data?.result ?? payload?.result ?? data?.output ?? payload?.output ?? data ?? payload;
        const failed = isFailedResult(payload);
        const targetId = ctx.resolveToolItemId(toolName, toolCallId);
        const toolCategory = resolveToolCategory(toolName, data ?? payload);
        const sandboxed = data?.sandbox === true;
        const outputKey = ctx.resolveToolOutputKey(toolName, toolCallId || targetId);
        const detailSource =
          data && typeof data === 'object'
            ? data
            : payload && typeof payload === 'object'
              ? payload
              : result;
        const detailPayload =
          sandboxed && detailSource && typeof detailSource === 'object' && !('sandbox' in detailSource)
            ? { ...detailSource, sandbox: true }
            : detailSource;
        const modelObservation = data?.model_observation ?? payload?.model_observation ?? null;
        const detailPayloadForDisplay =
          detailPayload && typeof detailPayload === 'object' && !Array.isArray(detailPayload)
            ? (() => {
                const next = { ...detailPayload };
                delete next.model_observation;
                return Object.keys(next).length > 0 ? next : null;
              })()
            : detailPayload;
        // Workflow detail should mirror model input first; fallback to raw payload only when observation is missing.
        const preferRawDetailForDisplay =
          typeof toolName === 'string'
          && (toolName.trim().toLowerCase() === 'apply_patch' || toolName.includes('应用补丁'));
        const detail = buildDetail(
          preferRawDetailForDisplay
            ? (detailPayloadForDisplay ?? modelObservation ?? result)
            : (modelObservation ?? detailPayloadForDisplay ?? result)
        );
        const timingMeta = buildWorkflowTimingMeta(detailPayload, result, data, payload);
        if (ctx.isSubagentControlTool(toolName)) {
          const subagentSource =
            result && typeof result === 'object' && !Array.isArray(result)
              ? result
              : detailPayload && typeof detailPayload === 'object' && !Array.isArray(detailPayload)
                ? detailPayload
                : data ?? payload;
          const subagentStatus =
            (subagentSource as Record<string, unknown>)?.state ??
            (subagentSource as Record<string, unknown>)?.status ??
            (failed ? 'error' : 'accepted');
          ctx.upsertSubagentPayloads(
            subagentSource,
            t('chat.workflow.event', { event: 'subagent_control' }),
            normalizeSubagentEventStatus(subagentStatus),
            { eventType: 'tool_result', source: subagentSource }
          );
          sessionSubagentsCache.delete(ctx.processorSessionId);
        }
        const commandSessionRows = ctx.isExecuteCommandTool(toolName)
          ? ctx.extractCommandSessionResultRows(detailPayload ?? result)
          : [];
        if (commandSessionRows.length > 0) {
          commandSessionRows.forEach(({ commandSessionId, source }) => {
            if (!commandSessionId) {
              return;
            }
            const exitCode = toOptionalInt(
              source?.returncode,
              source?.exit_code,
              source?.exitCode
            );
            const rowFailed =
              source?.timed_out === true
              || Boolean(pickString(source?.error))
              || exitCode === null
              || exitCode !== 0;
            ctx.syncCommandSessionSnapshot({
              ...source,
              command_session_id: commandSessionId,
              status: 'exited',
              exit_code: exitCode
            });
            ctx.ensureCommandSessionCallItem(
              commandSessionId,
              { ...source, status: 'exited', exit_code: exitCode },
              source?.command ?? ''
            );
            const outputKey = ctx.resolveToolOutputKey(
              ctx.executeCommandToolName,
              commandSessionId,
              commandSessionId
            );
            const outputBuffer = ctx.getToolOutputBuffer(outputKey);
            if (source?.command && !outputBuffer.command) {
              outputBuffer.command = String(source.command);
            }
            if (source?.stdout && !outputBuffer.stdout) {
              outputBuffer.stdout = String(source.stdout);
            }
            if (source?.stderr && !outputBuffer.stderr) {
              outputBuffer.stderr = String(source.stderr);
            }
            const outputItemId = ctx.ensureToolOutputItem(
              ctx.executeCommandToolName,
              outputKey,
              toolCategory,
              commandSessionId,
              ctx.buildCommandSessionTitle(
                source?.command,
                toOptionalInt(source?.command_index, source?.commandIndex)
              ),
              {
                commandSessionId,
                ...buildToolIdentityMeta(data, payload, source)
              }
            );
            if (outputItemId) {
              ctx.clearToolOutputFlush(outputKey);
            }
            ctx.upsertCommandSessionResultItem(commandSessionId, source, rowFailed);
            ctx.finalizeToolOutputItem(outputKey, rowFailed);
          });
          if (targetId) {
            updateWorkflowItem(ctx.assistantMessage.workflowItems, targetId, {
              status: failed ? 'failed' : 'completed'
            });
          }
          ctx.assistantMessage.workflowItems.push(
            buildWorkflowItem(
              `工具结果：${toolName || '未知工具'}`,
              detail,
              failed ? 'failed' : 'completed',
              {
                isTool: true,
                toolCategory,
                eventType: 'tool_result',
                toolName: String(toolName || ''),
                toolCallId: toolCallId || undefined,
                ...buildToolIdentityMeta(data, payload, detailPayload),
                ...buildWorkflowUsageMeta(detailPayload, result, data, payload),
                ...timingMeta
              }
            )
          );
          break;
        }
        if (targetId) {
          updateWorkflowItem(ctx.assistantMessage.workflowItems, targetId, {
            status: failed ? 'failed' : 'completed'
          });
        }
        ctx.finalizeToolOutputItem(outputKey, failed);
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(
            `工具结果：${toolName || '未知工具'}`,
            detail,
            failed ? 'failed' : 'completed',
            {
              isTool: true,
              toolCategory,
              eventType: 'tool_result',
              toolName: String(toolName || ''),
              toolCallId: toolCallId || undefined,
              ...buildToolIdentityMeta(data, payload, detailPayload),
              ...buildWorkflowUsageMeta(detailPayload, result, data, payload),
              ...timingMeta
            }
          )
        );
        if (!ctx.assistantMessage.questionPanel && isQuestionPanelToolName(toolName)) {
          const panelPayload = data?.data ?? data?.result ?? data?.output ?? null;
          ctx.applyQuestionPanelPayload(panelPayload);
        }
        break;
      }
      case 'approval_request': {
        const approvalId = String(data?.approval_id ?? payload?.approval_id ?? '').trim();
        const toolName = String(data?.tool ?? payload?.tool ?? '').trim();
        const toolCallId = ctx.extractToolCallRef(payload, data);
        const toolItemId = ctx.peekToolItemId(toolName, toolCallId);
        const title = toolName ? `等待审批：${toolName}` : '等待审批';
        const item = buildWorkflowItem(title, buildDetail(data ?? payload), 'pending', {
          eventType: 'approval_request',
          approvalId: approvalId || undefined,
          toolCallId: toolCallId || undefined
        });
        ctx.assistantMessage.workflowItems.push(item);
        if (approvalId) {
          ctx.approvalItemMap.set(approvalId, item.id);
        }
        if (toolItemId) {
          updateWorkflowItem(ctx.assistantMessage.workflowItems, toolItemId, {
            status: 'pending'
          });
        }
        break;
      }
      case 'approval_result': {
        const approvalId = String(data?.approval_id ?? payload?.approval_id ?? '').trim();
        const toolName = String(data?.tool ?? payload?.tool ?? '').trim();
        const toolCallId = ctx.extractToolCallRef(payload, data);
        const toolItemId = ctx.peekToolItemId(toolName, toolCallId);
        const statusRaw = String(data?.status ?? payload?.status ?? '').trim().toLowerCase();
        const itemStatus = statusRaw === 'approved' ? 'completed' : 'failed';
        const targetId = approvalId ? ctx.approvalItemMap.get(approvalId) : null;
        if (targetId) {
          updateWorkflowItem(ctx.assistantMessage.workflowItems, targetId, {
            status: itemStatus,
            detail: buildDetail(data ?? payload),
            eventType: 'approval_result',
            toolCallId: toolCallId || undefined
          });
          ctx.approvalItemMap.delete(approvalId);
          if (toolItemId) {
            updateWorkflowItem(ctx.assistantMessage.workflowItems, toolItemId, {
              status: statusRaw === 'approved' ? 'loading' : 'failed'
            });
          }
        } else {
          ctx.assistantMessage.workflowItems.push(
            buildWorkflowItem('审批结果', buildDetail(data ?? payload), itemStatus)
          );
        }
        break;
      }
      case 'thread_control': {
        const action = String(data?.action ?? payload?.action ?? '').trim().toLowerCase();
        const target =
          data?.switch_session ??
          data?.switchSession ??
          data?.session ??
          payload?.switch_session ??
          payload?.switchSession ??
          payload?.session ??
          null;
        const targetTitle = String(target?.title ?? '').trim();
        const titleMap = {
          create: '已创建会话线程',
          switch: '已切换会话线程',
          back: '已返回父线程',
          update_title: '已更新线程标题',
          archive: '已归档会话线程',
          restore: '已恢复会话线程',
          set_main: '已设置主线程'
        };
        const baseTitle = titleMap[action] || '会话线程控制';
        const title = targetTitle ? `${baseTitle}：${targetTitle}` : baseTitle;
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(title, buildDetail(data ?? payload), 'completed')
        );
        if (typeof ctx.options.onThreadControl === 'function') {
          try {
            const maybePromise = ctx.options.onThreadControl(data ?? payload);
            if (maybePromise && typeof maybePromise === 'object' && 'catch' in maybePromise) {
              Promise.resolve(maybePromise).catch(() => {});
            }
          } catch (error) {
            // Ignore side-effect failures to avoid breaking stream rendering.
          }
        }
        break;
      }
      case 'workspace_update': {
        const sessionId = payload?.session_id ?? payload?.sessionId ?? null;
        const workspaceId =
          data?.workspace_id ??
          data?.workspaceId ??
          payload?.workspace_id ??
          payload?.workspaceId ??
          null;
        const agentId = data?.agent_id ?? data?.agentId ?? payload?.agent_id ?? payload?.agentId ?? '';
        const containerId =
          data?.container_id ??
          data?.containerId ??
          payload?.container_id ??
          payload?.containerId ??
          null;
        const treeVersion =
          data?.tree_version ?? data?.treeVersion ?? payload?.tree_version ?? payload?.treeVersion ?? null;
        const changedPaths = collectWorkspacePathHints(data, payload);
        emitWorkspaceRefresh({
          sessionId,
          workspaceId,
          agentId,
          containerId,
          treeVersion,
          reason: data?.reason || payload?.reason || 'workspace_update',
          ...(changedPaths.length ? { path: changedPaths[0], paths: changedPaths } : {})
        });
        break;
      }
      case 'plan_update': {
        const normalized = applyPlanUpdate(ctx.assistantMessage, data);
        if (normalized) {
          ctx.assistantMessage.workflowItems.push(
            buildWorkflowItem(
              '计划更新',
              buildDetail({ explanation: normalized.explanation, plan: normalized.steps })
            )
          );
        }
        break;
      }
      case 'question_panel': {
        const appendWorkflow = !ctx.assistantMessage.questionPanel;
        ctx.applyQuestionPanelPayload(data, { appendWorkflow });
        break;
      }
      case 'slow_client': {
        const capacity = data?.queue_capacity ?? payload?.queue_capacity ?? '-';
        ctx.assistantMessage.slow_client = true;
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.slowClient'),
            t('chat.workflow.slowClientDetail', { capacity }),
            'failed',
            { eventType: 'slow_client' }
          )
        );
        break;
      }
      case 'llm_output_delta': {
        const round = ctx.resolveRound(payload, data);
        clearAssistantRetryState(ctx.assistantMessage);
        if (round !== null) {
          ctx.lastRound = round;
          ctx.assistantMessage.stream_round = round;
        }
        if (round !== null && ctx.blockedRounds.has(round)) {
          break;
        }
        if (round !== null && ctx.visibleRound !== round) {
          if (ctx.visibleRound === null && ctx.outputContent) {
            ctx.visibleRound = round;
          } else {
            ctx.clearVisibleOutput();
            ctx.visibleRound = round;
          }
        }
        const segmentedDelta = parseSegmentedDelta(payload, data);
        const delta =
          data?.delta ??
          payload?.delta ??
          data?.content ??
          payload?.content ??
          segmentedDelta?.delta ??
          '';
        const reasoningDelta =
          data?.reasoning_delta ??
          payload?.reasoning_delta ??
          data?.think_delta ??
          payload?.think_delta ??
          segmentedDelta?.reasoningDelta ??
          '';
        const reasoningDeltaText =
          typeof reasoningDelta === 'string' ? reasoningDelta : reasoningDelta ? String(reasoningDelta) : '';
        if (reasoningDeltaText) {
          ctx.pendingReasoningExplicit += reasoningDeltaText;
          ctx.outputState.reasoningStreaming = true;
        }
        if (typeof delta === 'string' && delta) {
          const parsedDelta = ctx.thinkStreamParser.push(delta);
          if (parsedDelta.reasoning) {
            ctx.pendingReasoningFallback += parsedDelta.reasoning;
            ctx.outputState.reasoningStreaming = true;
          }
          if (parsedDelta.content) {
            ctx.pendingContent += parsedDelta.content;
            ctx.outputState.streaming = true;
          }
        }
        if (ctx.pendingContent || ctx.pendingReasoningExplicit || ctx.pendingReasoningFallback) {
          ctx.ensureOutputItem();
          ctx.scheduleStreamFlush();
        }
        break;
      }
      case 'llm_output': {
        const round = ctx.resolveRound(payload, data);
        ctx.updatePartialConsumedFromUsage(data?.usage ?? payload?.usage ?? data, round);
        clearAssistantRetryState(ctx.assistantMessage);
        const outputPurpose = String(data?.purpose ?? payload?.purpose ?? '').trim().toLowerCase();
        if (outputPurpose === 'compaction_summary') {
          chatDebugLog('chat.compaction.event', 'llm-output-compaction-summary', {
            sessionId: ctx.options.sessionId ?? null,
            payload: cloneCompactionDebugPayload(data ?? payload ?? {}, {})
          });
          break;
        }
        ctx.updateUsageStats(
          data?.usage ?? payload?.usage ?? data,
          data?.prefill_duration_s ?? payload?.prefill_duration_s,
          data?.decode_duration_s ?? payload?.decode_duration_s,
          {
            round,
            accumulateDurations: true,
            includeInRoundAverage: true
          }
        );
        if (round !== null) {
          ctx.lastRound = round;
          ctx.assistantMessage.stream_round = round;
          ctx.applyModelRoundUsageToWorkflowTools(round, data ?? payload ?? {});
        }
        if (round !== null && ctx.blockedRounds.has(round)) {
          break;
        }
        if (round !== null && ctx.visibleRound !== round) {
          if (ctx.visibleRound === null && ctx.outputContent) {
            ctx.visibleRound = round;
          } else {
            ctx.clearVisibleOutput();
            ctx.visibleRound = round;
          }
        }
        ctx.flushStream(true);
        const content = data?.content ?? payload?.content ?? data?.output ?? payload?.output ?? '';
        const reasoningRaw =
          data?.reasoning ??
          payload?.reasoning ??
          data?.reasoning_content ??
          payload?.reasoning_content ??
          data?.think_content ??
          payload?.think_content ??
          '';
        const reasoningText = normalizeReasoningText(reasoningRaw);
        const parsedContent = normalizeAssistantOutput(content, reasoningText);
        const hasContent = typeof content === 'string' && content !== '';
        const toolCallsPayload =
          data?.tool_calls ??
          payload?.tool_calls ??
          data?.tool_call ??
          payload?.tool_call ??
          data?.function_call ??
          payload?.function_call;
        const toolCallAnswer = !hasContent ? extractFinalAnswerFromToolCalls(toolCallsPayload) : '';
        const parsedToolCallAnswer = normalizeAssistantOutput(toolCallAnswer, '');
        const resolvedContent = hasContent ? parsedContent.content : parsedToolCallAnswer.content;
        const inlineReasoning = hasContent
          ? parsedContent.inlineReasoning
          : parsedToolCallAnswer.inlineReasoning;
        const resolvedHasContent =
          typeof resolvedContent === 'string' && resolvedContent !== '';
        const hasReasoning = reasoningText !== '' || inlineReasoning !== '';
        if (resolvedHasContent || hasReasoning) {
          ctx.assistantMessage.manual_compaction_marker = false;
        }
        if (
          !resolvedHasContent &&
          !hasReasoning &&
          (ctx.outputState.streaming || ctx.outputState.reasoningStreaming)
        ) {
          ctx.outputState.streaming = false;
          ctx.outputState.reasoningStreaming = false;
        } else {
          if (reasoningText) {
            ctx.outputReasoningExplicit = reasoningText;
          } else if (inlineReasoning) {
            ctx.outputReasoningFallback = inlineReasoning;
          }
          if (resolvedHasContent) {
            ctx.outputContent = resolvedContent;
            ctx.assistantMessage.content = resolvedContent;
          }
          if (resolvedHasContent || hasReasoning) {
            markAssistantWaitingOutputVisible(ctx.assistantMessage);
          }
          ctx.outputState.streaming = false;
          ctx.outputState.reasoningStreaming = false;
        }
        ctx.syncReasoningToMessage();
        const outputId = ctx.ensureOutputItem();
        updateWorkflowItem(ctx.assistantMessage.workflowItems, outputId, {
          status: 'completed',
          detail: ctx.buildOutputDetail()
        });
        break;
      }
      case 'token_usage': {
        const round = ctx.resolveRound(payload, data);
        const usagePayload = data?.usage ?? payload?.usage ?? data;
        ctx.updatePartialConsumedFromUsage(usagePayload, round);
        ctx.updateUsageStats(
          usagePayload,
          null,
          null,
          {
            round,
            updateUsage: true,
            includeInRoundAverage: true
          }
        );
        ctx.updateLiveContextUsageFromTokenUsage(usagePayload, data ?? payload ?? {});
        ctx.applyModelRoundUsageToWorkflowTools(round, data ?? payload ?? {});
        break;
      }
      case 'round_usage': {
        if (isCompactionSummaryEvent('llm_output', data ?? payload ?? {})) {
          break;
        }
        const round = ctx.resolveRound(payload, data);
        ctx.updateRoundUsageStats(data ?? payload ?? {});
        ctx.updateUsageStats(
          data?.usage ?? payload?.usage ?? data ?? payload,
          null,
          null,
          // round_usage is the final request-level occupancy ctx.snapshot; keep llm_output usage for speed details.
          {
            round,
            updateUsage: !ctx.stats?.usage,
            includeInRoundAverage: false
          }
        );
        ctx.fallbackQuotaUsageFromRound(data ?? payload ?? {}, round);
        break;
      }
      case 'context_usage': {
        if (isCompactionSummaryEvent('llm_output', data ?? payload ?? {})) {
          break;
        }
        ctx.updateContextUsage(data ?? payload ?? {});
        if (ctx.activeCompactionWorkflowRef || ctx.compactionProgressItemMap.size > 0) {
          chatDebugLog('chat.compaction.event', 'context-usage', {
            sessionId: ctx.options.sessionId ?? null,
            activeWorkflowRef: ctx.activeCompactionWorkflowRef,
            trackedWorkflowRefs: Array.from(ctx.compactionProgressItemMap.keys()),
            payload: cloneCompactionDebugPayload(data ?? payload ?? {}, {})
          });
        }
        break;
      }
      case 'compaction': {
        ctx.markManualCompactionMarker(data ?? payload ?? {});
        const round = ctx.resolveRound(payload, data);
        const workflowRef = ctx.resolveStandaloneCompactionWorkflowRef(round, data ?? payload ?? {});
        const normalizedCompactionStatus = String(data?.status ?? payload?.status ?? '').trim().toLowerCase();
        const compactionStatus =
          normalizedCompactionStatus === 'failed' || normalizedCompactionStatus === 'error'
            ? 'failed'
            : 'completed';
        const finalized = ctx.finalizeCompactionProgressItem(
          workflowRef,
          data ?? payload ?? {},
          compactionStatus
        );
        if (!finalized) {
          ctx.assistantMessage.workflowItems.push(
            buildWorkflowItem(t('chat.toolWorkflow.compaction.title'), buildDetail(data ?? payload), compactionStatus, {
            isTool: true,
            eventType: 'compaction',
            toolName: '上下文压缩',
            toolCallId: workflowRef || undefined
          })
          );
          ctx.appendCompactionOutcomeNotice(workflowRef, data ?? payload ?? {}, compactionStatus);
          ctx.clearCompactionProgressRef(workflowRef);
        }
        chatDebugLog('chat.compaction.event', 'final', {
          sessionId: ctx.options.sessionId ?? null,
          round,
          workflowRef,
          finalizedFromProgress: finalized,
          status: compactionStatus,
          payload: cloneCompactionDebugPayload(data ?? payload ?? {}, {})
        });
        break;
      }
      case 'quota_usage': {
        const round = ctx.resolveRound(payload, data);
        ctx.updateQuotaUsage(data ?? payload ?? {}, round);
        break;
      }
      case 'final': {
        ctx.flushStream(true);
        ctx.compactionTerminalStatusHint = 'completed';
        const finalPayload =
          (data && typeof data === 'object' ? data : null)
          ?? (payload && typeof payload === 'object' ? payload : null)
          ?? {};
        ctx.finalizeLingeringCompactionProgressItems(finalPayload, 'completed');
        const answer =
          data?.answer ??
          payload?.answer ??
          data?.content ??
          payload?.content ??
          data?.message ??
          payload?.message ??
          raw;
        if (answer) {
          const answerText = pickText(answer, ctx.assistantMessage.content);
          const normalizedAnswer = normalizeAssistantOutput(answerText, '');
          ctx.assistantMessage.content = normalizedAnswer.content;
          ctx.outputContent = normalizedAnswer.content;
          if (normalizedAnswer.content || normalizedAnswer.inlineReasoning) {
            markAssistantWaitingOutputVisible(ctx.assistantMessage);
          }
          if (normalizedAnswer.inlineReasoning) {
            ctx.outputReasoningFallback = normalizedAnswer.inlineReasoning;
          }
          ctx.visibleRound = ctx.lastRound ?? ctx.visibleRound;
        }
        const stopReasonRaw = data?.stop_reason ?? payload?.stop_reason;
        const stopReason = ctx.normalizeStopReason(stopReasonRaw);
        if (stopReason) {
          ctx.assistantMessage.stop_reason = stopReason;
        }
        if (ctx.isToolFailureGuardStopReason(stopReason) && !ctx.toolFailureGuardNotified) {
          const stopMeta =
            (data?.stop_meta && typeof data.stop_meta === 'object' ? data.stop_meta : null) ??
            (payload?.stop_meta && typeof payload.stop_meta === 'object'
              ? payload.stop_meta
              : null);
          ctx.appendToolFailureGuardWorkflowItem(stopMeta ?? {}, 0);
        }
        if (ctx.lastRound !== null) {
          ctx.assistantMessage.stream_round = ctx.lastRound;
        }
        ctx.updateRoundUsageStats(finalPayload ?? data ?? payload ?? {});
        ctx.updateBackendTurnSpeedStats(finalPayload);
        ctx.outputState.streaming = false;
        ctx.outputState.reasoningStreaming = false;
        ctx.syncReasoningToMessage();
        const keepMarkerLayout = ctx.shouldKeepCompactionMarkerLayout();
        const hasOutputTrace = Boolean(
          String(ctx.outputContent || '').trim() || String(ctx.resolveReasoningOutput() || '').trim()
        );
        if (!keepMarkerLayout || ctx.outputItemId || hasOutputTrace) {
          const outputId = ctx.ensureOutputItem();
          updateWorkflowItem(ctx.assistantMessage.workflowItems, outputId, {
            status: 'completed',
            detail: ctx.buildOutputDetail()
          });
        }
        if (!keepMarkerLayout) {
          ctx.assistantMessage.workflowItems.push(
            buildWorkflowItem('最终回复', buildDetail(data || answer))
          );
        }
        break;
      }
      case 'turn_terminal': {
        ctx.flushStream(true);
        const terminalPayload =
          (data && typeof data === 'object' ? data : null)
          ?? (payload && typeof payload === 'object' ? payload : null)
          ?? {};
        const terminalStatus = String(terminalPayload?.status ?? '').trim().toLowerCase();
        const finalOk = terminalPayload?.final_ok;
        const terminalFailed =
          terminalStatus === 'failed' ||
          terminalStatus === 'error' ||
          terminalStatus === 'aborted' ||
          terminalStatus === 'cancelled' ||
          terminalStatus === 'canceled' ||
          finalOk === false;
        ctx.compactionTerminalStatusHint = terminalFailed ? 'failed' : 'completed';
        ctx.finalizeLingeringCompactionProgressItems(
          terminalPayload,
          terminalFailed ? 'failed' : 'completed'
        );
        const stopReasonRaw = terminalPayload?.stop_reason;
        const stopReason = ctx.normalizeStopReason(stopReasonRaw);
        if (stopReason) {
          ctx.assistantMessage.stop_reason = stopReason;
        }
        if (ctx.lastRound !== null) {
          ctx.assistantMessage.stream_round = ctx.lastRound;
        }
        ctx.outputState.streaming = false;
        ctx.outputState.reasoningStreaming = false;
        ctx.syncReasoningToMessage();
        const hasOutputTrace = Boolean(
          String(ctx.outputContent || '').trim() || String(ctx.resolveReasoningOutput() || '').trim()
        );
        if (ctx.outputItemId || hasOutputTrace) {
          const outputId = ctx.ensureOutputItem();
          updateWorkflowItem(ctx.assistantMessage.workflowItems, outputId, {
            status: terminalFailed ? 'failed' : 'completed',
            detail: ctx.buildOutputDetail()
          });
        }
        if (terminalFailed && !ctx.assistantMessage.content) {
          if (!ctx.assistantMessage.manual_compaction_marker) {
            const terminalDetail = pickText(
              terminalPayload?.message ?? terminalPayload?.error,
              t('chat.error.retry')
            );
            ctx.assistantMessage.content = terminalDetail;
          }
        }
        break;
      }
      case 'error': {
        ctx.compactionTerminalStatusHint = 'failed';
        const detail = data?.message ?? payload?.message ?? raw ?? t('chat.error.generic');
        const errorPayload = data && typeof data === 'object'
          ? data
          : payload && typeof payload === 'object'
            ? payload
            : { message: detail };
        const errorCode = String(data?.code ?? payload?.code ?? '').trim().toUpperCase();
        if (errorCode === 'CONTEXT_WINDOW_EXCEEDED' || ctx.isContextOverflowText(detail)) {
          ctx.markCompactionProgressFailed({
            ...((errorPayload && typeof errorPayload === 'object') ? errorPayload : {}),
            error_code: errorCode || 'CONTEXT_WINDOW_EXCEEDED',
            error_message: String(detail || '')
          });
        }
        ctx.finalizeLingeringCompactionProgressItems(errorPayload, 'failed');
        if (!ctx.assistantMessage.manual_compaction_marker) {
          ctx.assistantMessage.workflowItems.push(
            buildWorkflowItem(t('chat.workflow.error'), pickText(detail), 'failed', {
              eventType: 'error'
            })
          );
        }
        if (!ctx.assistantMessage.content && !ctx.assistantMessage.manual_compaction_marker) {
          ctx.assistantMessage.content = pickText(detail, t('chat.error.retry'));
        }
        break;
      }
      case 'subagent_dispatch_start': {
        const source = data ?? payload ?? {};
        const dispatchId = String(source?.dispatch_id ?? source?.dispatchId ?? '').trim();
        const label = String(source?.label ?? '').trim();
        const title = label ? `子智能体调度：${label}` : '子智能体调度';
        const detail = buildDetail(source);
        ctx.ensureSubagentDispatchItem(dispatchId, title, detail, 'loading');
        sessionSubagentsCache.delete(ctx.processorSessionId);
        break;
      }
      case 'subagent_dispatch_item_update': {
        const source = data ?? payload ?? {};
        const label = String(source?.label ?? source?.spawn_label ?? source?.title ?? '').trim();
        const titleBase =
          label ||
          String(source?.session_id ?? source?.sessionId ?? source?.run_id ?? source?.runId ?? '').trim() ||
          '任务';
        const title = `子智能体：${titleBase}`;
        ctx.upsertSubagentPayloads(
          source,
          title,
          ctx.normalizeSubagentWorkflowStatus(source?.status),
          { eventType: 'subagent_dispatch_item_update', source }
        );
        sessionSubagentsCache.delete(ctx.processorSessionId);
        break;
      }
      case 'subagent_dispatch_finish': {
        const source = data ?? payload ?? {};
        const dispatchId = String(source?.dispatch_id ?? source?.dispatchId ?? '').trim();
        const title = '子智能体调度结果';
        const detail = buildDetail(source);
        const status = ctx.normalizeSubagentWorkflowStatus(source?.status);
        ctx.ensureSubagentDispatchItem(dispatchId, title, detail, status);
        ctx.upsertSubagentPayloads(source, title, normalizeSubagentEventStatus(source?.status), {
          eventType: 'subagent_dispatch_finish',
          source
        });
        sessionSubagentsCache.delete(ctx.processorSessionId);
        break;
      }
      case 'subagent_status':
      case 'subagent_interrupt':
      case 'subagent_close':
      case 'subagent_resume':
      case 'subagent_announce': {
        const source = data ?? payload ?? raw;
        const titleMap = {
          subagent_status: '子智能体状态',
          subagent_interrupt: '子智能体中断',
          subagent_close: '子智能体关闭',
          subagent_resume: '子智能体恢复',
          subagent_announce: '子智能体回执'
        };
        const sourceObject = source && typeof source === 'object' ? source : {};
        ctx.upsertSubagentPayloads(
          sourceObject,
          titleMap[eventType] || '子智能体事件',
          ctx.normalizeSubagentWorkflowStatus((data ?? payload ?? {})?.status),
          { eventType, source: sourceObject }
        );
        sessionSubagentsCache.delete(ctx.processorSessionId);
        break;
      }
      case 'team_start':
      case 'team_task_dispatch':
      case 'team_task_update':
      case 'team_task_result':
      case 'team_merge':
      case 'team_finish':
      case 'team_error': {
        const status = eventType === 'team_error' ? 'failed' : eventType === 'team_finish' ? 'completed' : 'loading';
        ctx.assistantMessage.workflowItems.push(
          buildWorkflowItem(t('chat.workflow.event', { event: eventType }), buildDetail(data || raw), status)
        );
        if (eventType === 'team_task_result' || eventType === 'team_finish' || eventType === 'team_error') {
          const workerAgentId = String(data?.agent_id ?? payload?.agent_id ?? '').trim();
          emitAgentRuntimeRefresh({
            agentIds: workerAgentId ? [workerAgentId] : undefined
          });
        }
        break;
      }
      default: {
        if (eventType === 'llm_response') {
          const responsePurpose = String(data?.purpose ?? payload?.purpose ?? '').trim().toLowerCase();
          if (responsePurpose === 'compaction_summary') {
            chatDebugLog('chat.compaction.event', 'llm-response-compaction-summary', {
              sessionId: ctx.options.sessionId ?? null,
              payload: cloneCompactionDebugPayload(data ?? payload ?? {}, {})
            });
            break;
          }
        }
        const fallbackName = data?.name ?? payload?.name;
        const summary = fallbackName
          ? t('chat.workflow.eventWithName', { event: eventType, name: fallbackName })
          : t('chat.workflow.event', { event: eventType });
        ctx.assistantMessage.workflowItems.push(buildWorkflowItem(summary, buildDetail(data || raw)));
        break;
      }
    }
    ctx.notifySnapshot();
};
