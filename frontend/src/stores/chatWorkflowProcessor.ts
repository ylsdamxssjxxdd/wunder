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
import { resolveSessionKey, sessionSubagentsCache } from './chatRuntimeState';
import { buildWorkflowModelRoundUsageMeta, buildWorkflowTimingMeta, buildWorkflowUsageMeta, clearAssistantRetryState, collectSubagentPayloads, collectWorkspacePathHints, combineWorkflowUsageMeta, ensureMessageStats, estimateStreamOutputTokens, hasWorkflowUsageConsumedTokens, markAssistantRetryState, markAssistantWaitingOutputVisible, mergeWorkflowUsageSnapshot, normalizeContextTokens, normalizeContextTotalTokens, normalizeDurationValue, normalizeMessageSubagents, normalizeQuotaSnapshot, normalizeSpeedValue, normalizeStatsCount, normalizeSubagentEventStatus, normalizeUsagePayload, parseOptionalCount, resetAssistantWaitingOutputPhase, resolveContextPreviewTokens, resolveExplicitContextTokens, resolveInteractionDuration, resolveTimestampMs, resolveUsageConsumedTokensFromPayload, summarizeWorkflowUsageDebug, touchAssistantWaitingActivity, upsertMessageSubagent } from './chatStats';
import { normalizeFlag, normalizeStreamRound, parseSegmentedDelta, readDeltaSegments } from './chatStreamIds';
import { NormalizedUsagePayload, QuestionPanelApplyOptions, UsageStatsOptions, WorkflowProcessorOptions } from './chatTypes';
import { buildDetail, cloneCompactionDebugPayload, createThinkTagStreamParser, extractFinalAnswerFromToolCalls, isFailedResult, normalizeAssistantOutput, normalizeReasoningText, normalizeSessionWorkflowState, pickString, pickText, resolveAssistantReasoning, resolveEventType, resolveToolCategory, toOptionalInt, updateWorkflowItem } from './chatWorkflowHydration';
import { handleWorkflowProcessorEvent } from './chatWorkflowProcessorEvents';

export const createWorkflowProcessor = (assistantMessage, workflowState, onSnapshot, options: WorkflowProcessorOptions = {}) => {
  const roundState = normalizeSessionWorkflowState(workflowState);
  const perfEnabled = chatPerf.enabled();
  const commandSessionStore =
    typeof options.commandSessionStore === 'object' && options.commandSessionStore
      ? options.commandSessionStore
      : useCommandSessionStore();
  const processorSessionId = resolveSessionKey(options.sessionId);
  const toolItemMap = new Map();
  const toolCallItemMap = new Map();
  const approvalItemMap = new Map();
  const toolOutputItemMap = new Map();
  const toolOutputBufferMap = new Map();
  const toolOutputFlushTimerMap = new Map();
  const commandSessionResultItemMap = new Map();
  const compactionProgressItemMap = new Map();
  const subagentDispatchItemMap = new Map();
  const subagentRunItemMap = new Map();
  const roundMetricsMap = new Map<
    number,
    { prefill: number | null; decode: number | null; usage: NormalizedUsagePayload | null }
  >();
  const partialConsumedRoundMap = new Map<number, number>();
  const modelRoundUsagePayloadMap = new Map<number, unknown>();
  let outputItemId = null;
  const blockedRounds = new Set();
  const consumedQuotaRoundSet = new Set<number>();
  let toolFailureGuardNotified = false;
  let lastRound = null;
  let activeCompactionWorkflowRef = null;
  let compactionAnonymousRefSeq = 0;
  let compactionTerminalStatusHint: 'completed' | 'failed' | null = null;
  const initialRound = normalizeStreamRound(assistantMessage.stream_round);
  let visibleRound = initialRound;
  // 参照调试面板：记录模型输出轮次与内容，方便还原事件日志
  const outputState = {
    streaming: false,
    reasoningStreaming: false
  };
  const finalizeWithNow = options.finalizeWithNow !== false;
  const initialReasoning = resolveAssistantReasoning(assistantMessage);
  const normalizedOutput = normalizeAssistantOutput(
    assistantMessage.content,
    initialReasoning
  );
  const hasExplicitInitialReasoning = initialReasoning !== '';
  assistantMessage.content = normalizedOutput.content;
  // 思考内容需要同步到消息头部展示
  assistantMessage.reasoning = normalizedOutput.reasoning;
  assistantMessage.reasoningStreaming = normalizeFlag(assistantMessage.reasoningStreaming);
  const normalizedPlan = normalizePlanPayload(assistantMessage.plan);
  assistantMessage.plan = normalizedPlan;
  assistantMessage.planVisible =
    Boolean(assistantMessage.planVisible) || shouldAutoShowPlan(normalizedPlan, assistantMessage);
  assistantMessage.questionPanel = normalizeInquiryPanelState(assistantMessage.questionPanel);
  assistantMessage.subagents = normalizeMessageSubagents(assistantMessage.subagents);
  const stats = ensureMessageStats(assistantMessage);
  if (stats) {
    const seededRound = initialRound ?? 1;
    const seededPrefill = normalizeDurationValue(stats.prefill_duration_s);
    const seededDecode = normalizeDurationValue(stats.decode_duration_s);
    const seededUsage = normalizeUsagePayload(stats.usage);
    if (
      (seededPrefill !== null || seededDecode !== null || seededUsage) &&
      Number.isFinite(seededRound) &&
      seededRound > 0
    ) {
      roundMetricsMap.set(seededRound, {
        prefill: seededPrefill,
        decode: seededDecode,
        usage: seededUsage
      });
    }
    const seededPartialConsumed = normalizeStatsCount(stats.partialQuotaConsumed);
    if (seededRound > 0 && seededPartialConsumed > 0) {
      partialConsumedRoundMap.set(seededRound, seededPartialConsumed);
    }
  }
  let contextEstimateBaseTokens =
    normalizeContextTokens(options.initialContextTokens) ?? normalizeContextTokens(stats?.contextTokens);
  const refreshInteractionDuration = () => {
    if (!stats) return;
    const duration = resolveInteractionDuration(
      stats.interaction_start_ms,
      stats.interaction_end_ms
    );
    if (duration !== null) {
      stats.interaction_duration_s = duration;
    }
  };
  const applyInteractionTimestamp = (timestamp) => {
    if (!stats) return;
    const millis = resolveTimestampMs(timestamp);
    if (!Number.isFinite(millis)) {
      return;
    }
    if (!Number.isFinite(stats.interaction_start_ms)) {
      stats.interaction_start_ms = millis;
    } else {
      stats.interaction_start_ms = Math.min(stats.interaction_start_ms, millis);
    }
    if (!Number.isFinite(stats.interaction_end_ms)) {
      stats.interaction_end_ms = millis;
    } else {
      stats.interaction_end_ms = Math.max(stats.interaction_end_ms, millis);
    }
    refreshInteractionDuration();
  };
  const ensureInteractionStart = () => {
    if (!stats) return;
    if (!Number.isFinite(stats.interaction_start_ms)) {
      stats.interaction_start_ms = Date.now();
    }
  };
  const finalizeInteractionDuration = () => {
    if (!stats) return;
    if (!finalizeWithNow) {
      refreshInteractionDuration();
      return;
    }
    ensureInteractionStart();
    const now = Date.now();
    if (!Number.isFinite(stats.interaction_end_ms)) {
      stats.interaction_end_ms = now;
    } else {
      stats.interaction_end_ms = Math.max(stats.interaction_end_ms, now);
    }
    refreshInteractionDuration();
  };
  let outputContent = assistantMessage.content || '';
  let outputReasoningExplicit = hasExplicitInitialReasoning ? initialReasoning : '';
  let outputReasoningFallback = hasExplicitInitialReasoning
    ? normalizeReasoningText(normalizedOutput.inlineReasoning)
    : normalizeReasoningText(normalizedOutput.reasoning);
  const existingOutput = assistantMessage.workflowItems?.find((item) => item.title === '模型输出');
  if (existingOutput) {
    outputItemId = existingOutput.id;
  }

  const resolveReasoningOutput = () => outputReasoningExplicit || outputReasoningFallback;

  const syncReasoningToMessage = () => {
    assistantMessage.reasoning = resolveReasoningOutput();
    assistantMessage.reasoningStreaming = outputState.reasoningStreaming;
  };

  const normalizeToolName = (title) => {
    if (!title) return '';
    if (title.startsWith('调用工具：')) {
      return title.replace('调用工具：', '').trim();
    }
    return '';
  };

  const resolveWorkflowItemToolName = (item) => {
    const direct = String(item?.toolName || '').trim();
    if (direct) return direct;
    return normalizeToolName(item?.title);
  };

  const executeCommandToolName = 'execute_command';

  const isExecuteCommandTool = (toolName) => {
    const normalized = String(toolName || '').trim().toLowerCase();
    return normalized === executeCommandToolName || normalized.includes('执行命令');
  };

  const isSubagentControlTool = (toolName) => {
    const normalized = String(toolName || '').trim().toLowerCase();
    return normalized.includes('subagent') || normalized.includes('child_agent') || normalized.includes('子智能体');
  };

  const normalizeCommandSessionRef = (value) => {
    const normalized = String(value || '').trim();
    return normalized || null;
  };

  const extractCommandSessionRef = (payload, data) => {
    for (const source of [data, payload]) {
      if (!source || typeof source !== 'object') continue;
      const ref = normalizeCommandSessionRef(
        source.command_session_id ?? source.commandSessionId
      );
      if (ref) return ref;
    }
    return null;
  };

  const syncCommandSessionSnapshot = (source) => {
    if (!source || typeof source !== 'object') return null;
    const record = source;
    return commandSessionStore.upsertSnapshot(
      processorSessionId || record.session_id || record.sessionId || '',
      record
    );
  };

  const syncCommandSessionDelta = (commandSessionId, stream, delta, meta = {}) =>
    commandSessionStore.appendDelta(
      processorSessionId || '',
      commandSessionId,
      stream,
      delta,
      meta
    );

  const buildCommandSessionTitle = (command, commandIndex = null) => {
    const normalized = String(command || '')
      .replace(/\r\n/g, '\n')
      .replace(/\r/g, '\n');
    const firstLine = normalized
      .split('\n')
      .map((line) => line.trim())
      .find(Boolean);
    if (firstLine) {
      return firstLine.length > 96 ? `${firstLine.slice(0, 96)}...` : firstLine;
    }
    if (Number.isFinite(commandIndex)) {
      return `${t('chat.toolWorkflow.toolLabel.executeCommand')} #${Number(commandIndex) + 1}`;
    }
    return t('chat.toolWorkflow.toolLabel.executeCommand');
  };

  const normalizeStopReason = (value) => String(value || '').trim().toLowerCase();

  const isToolFailureGuardStopReason = (value) => {
    const normalized = normalizeStopReason(value);
    return normalized === 'tool_failure_guard' || normalized === 'tool-failure-guard';
  };

  const isManualCompactionDetail = (value): boolean => {
    if (!value || typeof value !== 'object') return false;
    const detail = value as Record<string, unknown>;
    const triggerMode = String(detail.trigger_mode ?? detail.triggerMode ?? '').trim().toLowerCase();
    return triggerMode === 'manual';
  };

  const shouldKeepManualCompactionMarkerLayout = (): boolean => {
    if (String(assistantMessage.content || '').trim()) return false;
    if (String(resolveReasoningOutput() || '').trim()) return false;
    if (hasPlanSteps(assistantMessage.plan)) return false;
    const panelStatus = String(assistantMessage?.questionPanel?.status || '').trim().toLowerCase();
    return panelStatus !== 'pending';
  };

  const markManualCompactionMarker = (detailSource): void => {
    if (!isManualCompactionDetail(detailSource)) return;
    if (!shouldKeepManualCompactionMarkerLayout()) return;
    assistantMessage.manual_compaction_marker = true;
  };

  const parseOptionalPositiveCount = (value) => {
    const parsed = parseOptionalCount(value);
    if (parsed === null || parsed <= 0) return null;
    return parsed;
  };

  const buildToolFailureGuardNotice = (source, fallbackThreshold = null) => {
    const detailSource = source && typeof source === 'object' ? source : {};
    const tool = String(detailSource.tool ?? detailSource.tool_name ?? '').trim();
    const repeatCount = parseOptionalPositiveCount(
      detailSource.repeat_count ?? detailSource.repeatCount
    );
    const threshold = parseOptionalPositiveCount(
      detailSource.threshold ?? detailSource.max_repeat ?? fallbackThreshold
    );
    if (!repeatCount || !threshold) {
      return null;
    }
    if (tool) {
      return t('chat.workflow.toolFailureGuardDetail', {
        tool,
        repeatCount,
        threshold
      });
    }
    return t('chat.workflow.toolFailureGuardDetailNoTool', {
      repeatCount,
      threshold
    });
  };

  const appendToolFailureGuardWorkflowItem = (source, fallbackThreshold = null) => {
    const detailSource = source && typeof source === 'object' ? source : {};
    const toolName = String(detailSource.tool ?? detailSource.tool_name ?? '').trim();
    const notice = buildToolFailureGuardNotice(detailSource, fallbackThreshold);
    const rawError = detailSource.tool_error ?? detailSource.error ?? '';
    const toolError = typeof rawError === 'string' ? rawError.trim() : '';
    const detail = toolError ? `${notice || ''}${notice ? '\n\n' : ''}${toolError}` : notice || '';
    assistantMessage.workflowItems.push(
      buildWorkflowItem(
        t('chat.workflow.toolFailureGuardTriggered'),
        detail,
        'failed',
        {
          isTool: true,
          eventType: 'tool_result',
          toolName: toolName || t('chat.workflow.toolUnknown')
        }
      )
    );
    toolFailureGuardNotified = true;
  };

  const registerToolStats = (toolName) => {
    if (!stats) return;
    stats.toolCalls = normalizeStatsCount(stats.toolCalls) + 1;
  };

  const recomputeRoundAggregates = () => {
    if (!stats) return;
    const summary = summarizeTurnDecodeSpeed(roundMetricsMap.values());
    stats.prefill_duration_total_s = summary.prefillDurationTotalS;
    stats.decode_duration_total_s = summary.decodeDurationTotalS;
    if (summary.avgModelRoundSpeedTps !== null) {
      stats.avg_model_round_speed_tps = summary.avgModelRoundSpeedTps;
      stats.avg_model_round_speed_rounds = summary.avgModelRoundSpeedRounds;
      return;
    }
    if (normalizeSpeedValue(stats.avg_model_round_speed_tps) === null) {
      stats.avg_model_round_speed_tps = null;
    }
    if (normalizeStatsCount(stats.avg_model_round_speed_rounds) <= 0) {
      stats.avg_model_round_speed_rounds = 0;
    }
  };

  const updateBackendTurnSpeedStats = (source) => {
    if (!stats || !source || typeof source !== 'object') return;
    const prefillTotal = normalizeDurationValue(
      source.prefill_duration_total_s ?? source.prefillDurationTotalS
    );
    const decodeTotal = normalizeDurationValue(
      source.decode_duration_total_s ?? source.decodeDurationTotalS
    );
    const avgSpeed = normalizeSpeedValue(
      source.avg_model_round_speed_tps ??
        source.avg_model_round_decode_speed_tps ??
        source.avgModelRoundDecodeSpeedTps ??
        source.avgModelRoundSpeedTps ??
        source.average_speed_tps ??
        source.averageSpeedTps
    );
    const avgRounds = normalizeStatsCount(
      source.avg_model_round_speed_rounds ??
        source.avgModelRoundSpeedRounds ??
        source.average_speed_rounds ??
        source.averageSpeedRounds
    );
    if (prefillTotal !== null) {
      stats.prefill_duration_total_s = prefillTotal;
    }
    if (decodeTotal !== null) {
      stats.decode_duration_total_s = decodeTotal;
    }
    if (avgSpeed !== null) {
      stats.avg_model_round_speed_tps = avgSpeed;
    }
    if (avgRounds > 0) {
      stats.avg_model_round_speed_rounds = avgRounds;
    }
  };

  const recomputePartialConsumedTotal = () => {
    if (!stats) return;
    let total = 0;
    partialConsumedRoundMap.forEach((value) => {
      total += normalizeStatsCount(value);
    });
    stats.partialQuotaConsumed = total;
  };

  const updatePartialConsumedFromUsage = (usagePayload, roundValue) => {
    if (!stats) return;
    const roundNumber = normalizeStreamRound(roundValue);
    if (roundNumber === null) return;
    const consumed = resolveUsageConsumedTokensFromPayload(usagePayload);
    if (consumed <= 0) return;
    const previous = partialConsumedRoundMap.get(roundNumber) ?? 0;
    if (consumed <= previous) return;
    partialConsumedRoundMap.set(roundNumber, consumed);
    recomputePartialConsumedTotal();
  };

  const updateUsageStats = (
    usagePayload,
    prefillDuration,
    decodeDuration,
    usageOptions: UsageStatsOptions = {}
  ) => {
    if (!stats) return;
    const normalizedUsage = normalizeUsagePayload(usagePayload);
    const shouldUpdateUsage = Boolean(normalizedUsage && usageOptions.updateUsage !== false);
    const existingContextTokens = normalizeContextTokens(stats.contextTokens);
    if (shouldUpdateUsage) {
      stats.usage = normalizedUsage;
    }
    if (existingContextTokens !== null && existingContextTokens > 0) contextEstimateBaseTokens = existingContextTokens;
    const prefill = normalizeDurationValue(prefillDuration);
    const decode = normalizeDurationValue(decodeDuration);
    if (prefill !== null) {
      stats.prefill_duration_s = prefill;
    }
    if (decode !== null) {
      stats.decode_duration_s = decode;
    }
    const roundNumber = normalizeStreamRound(usageOptions.round);
    if (
      roundNumber !== null &&
      (usageOptions.accumulateDurations || usageOptions.includeInRoundAverage)
    ) {
      const current = roundMetricsMap.get(roundNumber) ?? {
        prefill: null,
        decode: null,
        usage: null
      };
      if (prefill !== null) {
        current.prefill = prefill;
      }
      if (decode !== null) {
        current.decode = decode;
      }
      if (normalizedUsage && usageOptions.includeInRoundAverage) {
        current.usage = normalizedUsage;
      }
      roundMetricsMap.set(roundNumber, current);
      recomputeRoundAggregates();
      return;
    }
  };

  const syncLiveContextTokens = (nextContextTokens, contextTotalTokens = null) => {
    if (!stats) return;
    const normalized = normalizeContextTokens(nextContextTokens);
    if (normalized === null || normalized <= 0) return;
    const normalizedTotal = normalizeContextTotalTokens(contextTotalTokens);
    const existingContextTokens = normalizeContextTokens(stats.contextTokens);
    const changed = existingContextTokens !== normalized;
    stats.contextTokens = normalized;
    contextEstimateBaseTokens = normalized;
    if (normalizedTotal !== null) {
      stats.contextTotalTokens = normalizedTotal;
    }
    if (changed || normalizedTotal !== null) {
      options.onContextUsage?.(normalized, stats.contextTotalTokens ?? null);
      notifySnapshot(true);
    }
  };

  const syncContextPreviewTokens = (nextContextTokens) => {
    if (!stats) return;
    const normalized = normalizeContextTokens(nextContextTokens);
    if (normalized === null || normalized <= 0) return;
    const existingContextTokens = normalizeContextTokens(stats.contextTokens);
    // Preview is only a composer fallback. Once observed occupancy exists, never
    // let estimates compete with the real session/bubble context value.
    if (existingContextTokens !== null && existingContextTokens > 0) {
      return;
    }
    const existingPreviewTokens = resolveContextPreviewTokens(stats);
    if (existingPreviewTokens === normalized) return;
    stats.contextPreviewTokens = normalized;
    notifySnapshot(true);
  };

  const resolveExplicitLiveContextTokens = (payload) => {
    return resolveExplicitContextTokens(payload);
  };

  const updateLiveContextUsageFromTokenUsage = (usagePayload, sourcePayload = usagePayload) => {
    const explicitContextTokens =
      resolveExplicitLiveContextTokens(sourcePayload) ??
      resolveExplicitLiveContextTokens(usagePayload);
    if (explicitContextTokens === null) return;
    const contextTotalTokens = normalizeContextTotalTokens(
      sourcePayload?.max_context ??
        sourcePayload?.maxContext ??
        sourcePayload?.context_total_tokens ??
        sourcePayload?.contextTotalTokens ??
        sourcePayload?.context_usage?.max_context ??
        sourcePayload?.context_usage?.context_max_tokens
    );
    syncLiveContextTokens(explicitContextTokens, contextTotalTokens);
  };

  const updateLiveContextUsageFromRequest = (requestPayload) => {
    const requestEstimateTokens = estimateRequestContextTokens(requestPayload);
    const confirmedContextTokens =
      normalizeContextTokens(contextEstimateBaseTokens) ?? normalizeContextTokens(stats?.contextTokens);
    const previewContextTokens = resolveRequestContextPreviewTokens(
      requestEstimateTokens,
      confirmedContextTokens
    );
    if (previewContextTokens !== null) {
      syncContextPreviewTokens(previewContextTokens);
    }
  };

  const updateRoundUsageStats = (usagePayload) => {
    if (!stats) return;
    const normalizedUsage = normalizeUsagePayload(usagePayload?.usage ?? usagePayload);
    if (!normalizedUsage) return;
    stats.roundUsage = normalizedUsage;
    const explicitContextTokens = resolveExplicitContextTokens(usagePayload);
    const explicitContextTotalTokens = normalizeContextTotalTokens(
      usagePayload?.max_context ??
        usagePayload?.maxContext ??
        usagePayload?.context_total_tokens ??
        usagePayload?.contextTotalTokens ??
        usagePayload?.context_usage?.max_context ??
        usagePayload?.context_usage?.context_max_tokens
    );
    if (explicitContextTokens !== null) {
      syncLiveContextTokens(explicitContextTokens, explicitContextTotalTokens);
    } else if (explicitContextTotalTokens !== null) {
      stats.contextTotalTokens = explicitContextTotalTokens;
    }
  };

  const markQuotaRoundConsumed = (roundNumber) => {
    if (!Number.isFinite(roundNumber)) return false;
    if (consumedQuotaRoundSet.has(roundNumber)) return false;
    consumedQuotaRoundSet.add(roundNumber);
    return true;
  };

  const updateQuotaUsage = (payload, roundNumber = null) => {
    if (!stats) return;
    const rawIncrement =
      payload && typeof payload === 'object'
        ? payload.request_consumed_tokens ??
          payload.requestConsumedTokens ??
          payload.consumed_tokens ??
          payload.consumedTokens ??
          payload.consumed ??
          payload.count ??
          payload.used
        : null;
    const increment = normalizeStatsCount(rawIncrement);
    if (increment > 0) {
      if (Number.isFinite(roundNumber) && !markQuotaRoundConsumed(roundNumber)) {
        return;
      }
      stats.quotaConsumed = normalizeStatsCount(stats.quotaConsumed) + increment;
    }
    const snapshot = normalizeQuotaSnapshot(payload);
    if (snapshot) {
      stats.quotaSnapshot = snapshot;
    }
  };

  const fallbackQuotaUsageFromRound = (payload, roundNumber) => {
    if (!stats) return;
    if (Number.isFinite(roundNumber)) {
      if (!markQuotaRoundConsumed(roundNumber)) {
        return;
      }
    }
    const increment = normalizeStatsCount(
      payload?.request_consumed_tokens ??
        payload?.requestConsumedTokens ??
        payload?.consumed_tokens ??
        payload?.consumedTokens ??
        payload?.consumed ??
        payload?.used ??
        payload?.count ??
        payload?.usage?.total ??
        payload?.usage?.total_tokens ??
        payload?.usage?.totalTokens ??
        payload?.total ??
        payload?.total_tokens ??
        payload?.totalTokens
    );
    if (increment > 0) {
      stats.quotaConsumed = normalizeStatsCount(stats.quotaConsumed) + increment;
    }
  };

  const updateContextUsage = (payload) => {
    if (!stats) return;
    const explicitContextTokens =
      resolveExplicitContextTokens(payload) ?? normalizeContextTokens(payload?.context);
    const contextTotalTokens = normalizeContextTotalTokens(
      payload?.max_context ??
        payload?.maxContext ??
        payload?.context_window ??
        payload?.context_max_tokens ??
        payload?.contextTotalTokens ??
        payload?.context_usage?.max_context ??
        payload?.context_usage?.context_max_tokens
    );
    if (explicitContextTokens !== null) {
      syncLiveContextTokens(explicitContextTokens, contextTotalTokens);
    } else if (contextTotalTokens !== null) {
      stats.contextTotalTokens = contextTotalTokens;
    }
  };

  const normalizeToolQueueKey = (toolName) => String(toolName || '').trim().toLowerCase();

  const normalizeToolCallRef = (value) => {
    const normalized = String(value || '').trim();
    return normalized || null;
  };

  const extractToolCallRef = (payload, data) => {
    for (const source of [data, payload]) {
      if (!source || typeof source !== 'object') continue;
      const ref = normalizeToolCallRef(
        source.tool_call_id ?? source.toolCallId ?? source.call_id ?? source.callId
      );
      if (ref) return ref;
    }
    return null;
  };

  const removeQueuedToolItem = (toolKey, itemId) => {
    if (!toolKey || !itemId) return;
    const queue = toolItemMap.get(toolKey);
    if (!queue?.length) return;
    const nextQueue = queue.filter((candidate) => candidate !== itemId);
    if (nextQueue.length > 0) {
      toolItemMap.set(toolKey, nextQueue);
    } else {
      toolItemMap.delete(toolKey);
    }
  };

  const registerToolItem = (toolName, itemId, toolCallId = null) => {
    const toolKey = normalizeToolQueueKey(toolName);
    if (!toolKey || !itemId) return;
    if (!toolItemMap.has(toolKey)) {
      toolItemMap.set(toolKey, []);
    }
    const queue = toolItemMap.get(toolKey);
    if (!queue.includes(itemId)) {
      queue.push(itemId);
    }
    const normalizedCallId = normalizeToolCallRef(toolCallId);
    if (normalizedCallId) {
      toolCallItemMap.set(normalizedCallId, itemId);
    }
  };

  const applyModelRoundUsageToWorkflowTools = (roundNumber, usagePayload) => {
    const normalizedRound = normalizeStreamRound(roundNumber);
    if (!Number.isFinite(normalizedRound) || !Array.isArray(assistantMessage.workflowItems)) {
      return;
    }
    modelRoundUsagePayloadMap.set(normalizedRound, usagePayload);
    const usageMeta = buildWorkflowModelRoundUsageMeta(usagePayload);
    if (!usageMeta.payload) {
      return;
    }
    const debugMatches = [];
    assistantMessage.workflowItems.forEach((item) => {
      if (!item || typeof item !== 'object') return;
      if (String(item.eventType || '').trim().toLowerCase() !== 'tool_call') return;
      if (normalizeStreamRound(item.modelRound ?? item.model_round ?? item.round) !== normalizedRound) {
        return;
      }
      const currentPayload = item.payload ?? item.meta ?? item;
      const hadConsumedTokens = hasWorkflowUsageConsumedTokens(currentPayload);
      if (hadConsumedTokens) {
        if (isChatDebugEnabled()) {
          debugMatches.push({
            toolName: String(item.toolName || item.tool || ''),
            toolCallId: String(item.toolCallId || item.tool_call_id || item.callId || item.call_id || ''),
            action: 'skip-existing-consumed',
            before: summarizeWorkflowUsageDebug(currentPayload)
          });
        }
        return;
      }
      const mergedPayload = mergeWorkflowUsageSnapshot(item.payload, usageMeta.payload);
      if (mergedPayload) {
        item.payload = mergedPayload;
        if (isChatDebugEnabled()) {
          debugMatches.push({
            toolName: String(item.toolName || item.tool || ''),
            toolCallId: String(item.toolCallId || item.tool_call_id || item.callId || item.call_id || ''),
            action: 'apply-round-usage',
            before: summarizeWorkflowUsageDebug(currentPayload),
            applied: summarizeWorkflowUsageDebug(usageMeta.payload),
            after: summarizeWorkflowUsageDebug(mergedPayload)
          });
        }
        return;
      }
      Object.assign(item, usageMeta);
      if (isChatDebugEnabled()) {
        debugMatches.push({
          toolName: String(item.toolName || item.tool || ''),
          toolCallId: String(item.toolCallId || item.tool_call_id || item.callId || item.call_id || ''),
          action: 'assign-round-usage',
          applied: summarizeWorkflowUsageDebug(usageMeta.payload),
          after: summarizeWorkflowUsageDebug(item.payload ?? item.meta ?? item)
        });
      }
    });
    if (isChatDebugEnabled()) {
      chatDebugLog('chat.store.runtime', 'workflow-tool-model-usage', {
        roundNumber: normalizedRound,
        incoming: summarizeWorkflowUsageDebug(usageMeta.payload),
        matchedToolCalls: debugMatches.length,
        matches: debugMatches
      });
    }
  };

  const resolveToolItemId = (toolName, toolCallId = null) => {
    const normalizedCallId = normalizeToolCallRef(toolCallId);
    if (normalizedCallId) {
      const exact = toolCallItemMap.get(normalizedCallId);
      if (exact) {
        removeQueuedToolItem(normalizeToolQueueKey(toolName), exact);
        return exact;
      }
    }
    const toolKey = normalizeToolQueueKey(toolName);
    if (!toolKey) return null;
    const queue = toolItemMap.get(toolKey);
    if (!queue || queue.length === 0) return null;
    return queue.shift() || null;
  };

  const peekToolItemId = (toolName, toolCallId = null) => {
    const normalizedCallId = normalizeToolCallRef(toolCallId);
    if (normalizedCallId) {
      const exact = toolCallItemMap.get(normalizedCallId);
      if (exact) return exact;
    }
    const toolKey = normalizeToolQueueKey(toolName);
    if (!toolKey) return null;
    const queue = toolItemMap.get(toolKey);
    if (!queue || queue.length === 0) return null;
    return queue[0] || null;
  };

  const resolveToolOutputKey = (toolName, callId, commandSessionId = null) => {
    const normalizedCommandSessionId = normalizeCommandSessionRef(commandSessionId);
    if (normalizedCommandSessionId) {
      return `command_session:${normalizedCommandSessionId}`;
    }
    if (callId) return `call:${callId}`;
    if (toolName) return `tool:${toolName}`;
    return 'tool:unknown';
  };

  const getToolOutputBuffer = (key) => {
    let buffer = toolOutputBufferMap.get(key);
    if (!buffer) {
      buffer = { stdout: '', stderr: '', command: '', stdoutDropped: 0, stderrDropped: 0 };
      toolOutputBufferMap.set(key, buffer);
    }
    return buffer;
  };

  const TOOL_OUTPUT_MAX_CHARS = 20000;

  const appendToolOutput = (buffer, field, delta) => {
    if (!buffer || !delta) return;
    const current = String(buffer[field] || '');
    const next = current + delta;
    if (next.length <= TOOL_OUTPUT_MAX_CHARS) {
      buffer[field] = next;
      return;
    }
    const overflow = next.length - TOOL_OUTPUT_MAX_CHARS;
    buffer[field] = next.slice(overflow);
    const droppedKey = field === 'stderr' ? 'stderrDropped' : 'stdoutDropped';
    const dropped = Number.isFinite(buffer[droppedKey]) ? buffer[droppedKey] : 0;
    buffer[droppedKey] = dropped + overflow;
  };

  const buildToolOutputDetail = (buffer) => {
    if (!buffer) return '';
    const parts = [];
    if (buffer.command) {
      parts.push(`[command]\n${buffer.command}`);
    }
    if (buffer.stdout) {
      const dropped = Number.isFinite(buffer.stdoutDropped) ? buffer.stdoutDropped : 0;
      const prefix = dropped > 0 ? `... (truncated ${dropped} chars)\n` : '';
      parts.push(`[stdout]\n${prefix}${buffer.stdout}`);
    }
    if (buffer.stderr) {
      const dropped = Number.isFinite(buffer.stderrDropped) ? buffer.stderrDropped : 0;
      const prefix = dropped > 0 ? `... (truncated ${dropped} chars)\n` : '';
      parts.push(`[stderr]\n${prefix}${buffer.stderr}`);
    }
    return parts.join('\n\n');
  };

  const extractToolOutputSection = (detail, tag) => {
    const normalized = String(detail || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
    if (!normalized) return '';
    const pattern = new RegExp(
      `\\[${tag}\\]\\n([\\s\\S]*?)(?=\\n\\n\\[(?:command|stdout|stderr)\\]\\n|$)`,
      'i'
    );
    const match = normalized.match(pattern);
    return String(match?.[1] || '').trim();
  };
  const TOOL_OUTPUT_FLUSH_MS = 120;

  const clearToolOutputFlush = (key) => {
    if (!key) return;
    const timer = toolOutputFlushTimerMap.get(key);
    if (timer) {
      clearTimeout(timer);
      toolOutputFlushTimerMap.delete(key);
    }
  };

  const flushToolOutputDetail = (key, itemId) => {
    if (!key || !itemId) return;
    const buffer = toolOutputBufferMap.get(key);
    if (!buffer) return;
    updateWorkflowItem(assistantMessage.workflowItems, itemId, {
      detail: buildToolOutputDetail(buffer),
      status: 'loading'
    });
  };

  const scheduleToolOutputFlush = (key, itemId) => {
    if (!key || !itemId) return;
    if (toolOutputFlushTimerMap.has(key)) return;
    const timer = setTimeout(() => {
      toolOutputFlushTimerMap.delete(key);
      if (perfEnabled) {
        const start = performance.now();
        flushToolOutputDetail(key, itemId);
        chatPerf.recordDuration('chat_tool_output_flush', performance.now() - start);
      } else {
        flushToolOutputDetail(key, itemId);
      }
    }, TOOL_OUTPUT_FLUSH_MS);
    toolOutputFlushTimerMap.set(key, timer);
  };

  const flushAllToolOutputs = () => {
    toolOutputFlushTimerMap.forEach((timer, key) => {
      clearTimeout(timer);
      toolOutputFlushTimerMap.delete(key);
      const itemId = toolOutputItemMap.get(key);
      if (itemId) {
        flushToolOutputDetail(key, itemId);
      }
    });
  };

  const ensureToolOutputItem = (
    toolName,
    key,
    toolCategory,
    toolCallId = null,
    itemTitle = null,
    extraMeta = null
  ) => {
    if (!key) return null;
    const existing = toolOutputItemMap.get(key);
    if (existing) return existing;
    const title = toolName ? `工具输出：${toolName}` : '工具输出';
    const resolvedTitle = itemTitle || title;
    const item = buildWorkflowItem(resolvedTitle, '', 'loading', {
      isTool: true,
      toolCategory,
      eventType: 'tool_output_delta',
      toolName: String(toolName || ''),
      toolCallId: toolCallId || undefined,
      ...(extraMeta && typeof extraMeta === 'object' ? extraMeta : {})
    });
    assistantMessage.workflowItems.push(item);
    toolOutputItemMap.set(key, item.id);
    return item.id;
  };

  const finalizeToolOutputItem = (key, failed) => {
    if (!key) return;
    const itemId = toolOutputItemMap.get(key);
    if (!itemId) return;
    const buffer = toolOutputBufferMap.get(key);
    clearToolOutputFlush(key);
    updateWorkflowItem(assistantMessage.workflowItems, itemId, {
      status: failed ? 'failed' : 'completed',
      detail: buffer ? buildToolOutputDetail(buffer) : ''
    });
    toolOutputItemMap.delete(key);
    toolOutputBufferMap.delete(key);
  };

  const resolveCommandSessionStatus = (source, fallbackStatus = 'loading') => {
    const normalized = String(source?.status || '').trim().toLowerCase();
    if (normalized === 'running') {
      return 'loading';
    }
    if (normalized === 'failed_to_start') {
      return 'failed';
    }
    if (normalized === 'exited') {
      const exitCode = toOptionalInt(
        source?.returncode,
        source?.exit_code,
        source?.exitCode
      );
      const failed =
        source?.timed_out === true
        || Boolean(pickString(source?.error))
        || exitCode === null
        || exitCode !== 0;
      return failed ? 'failed' : 'completed';
    }
    return fallbackStatus;
  };

  const ensureCommandSessionCallItem = (commandSessionId, source, command = '') => {
    const normalizedSessionId = normalizeCommandSessionRef(commandSessionId);
    if (!normalizedSessionId) return null;
    const normalizedToolCallId = normalizeToolCallRef(
      source?.tool_call_id ?? source?.toolCallId ?? source?.call_id ?? source?.callId
    );
    const workflowRef = normalizedToolCallId || normalizedSessionId;
    const toolCategory = resolveToolCategory(executeCommandToolName, source);
    const commandIndex = toOptionalInt(source?.command_index, source?.commandIndex);
    const title = buildCommandSessionTitle(command || source?.command, commandIndex);
    const detail = buildDetail({
      ...(source && typeof source === 'object' ? source : {}),
      tool: executeCommandToolName,
      command: pickString(command, source?.command)
    });
    const usageMeta = combineWorkflowUsageMeta(
      buildWorkflowModelRoundUsageMeta(
        Number.isFinite(lastRound) ? modelRoundUsagePayloadMap.get(lastRound) : null
      ),
      buildWorkflowUsageMeta(source)
    );
    const patch = {
      title,
      detail,
      status: resolveCommandSessionStatus(source, 'loading'),
      isTool: true,
      toolCategory,
      eventType: 'tool_call',
      toolName: executeCommandToolName,
      toolCallId: workflowRef,
      commandSessionId: normalizedSessionId,
      ...buildToolIdentityMeta(source, {
        tool_display_name: executeCommandToolName
      }),
      modelRound: Number.isFinite(lastRound) ? lastRound : undefined,
      ...usageMeta,
      ...buildWorkflowTimingMeta(source)
    };
    const existing = toolCallItemMap.get(workflowRef) || toolCallItemMap.get(normalizedSessionId);
    if (existing) {
      updateWorkflowItem(assistantMessage.workflowItems, existing, patch);
      toolCallItemMap.set(normalizedSessionId, existing);
      if (normalizedToolCallId) {
        toolCallItemMap.set(normalizedToolCallId, existing);
      }
      return existing;
    }
    const item = buildWorkflowItem(title, detail, patch.status, {
      isTool: true,
      toolCategory,
      eventType: 'tool_call',
      toolName: executeCommandToolName,
      toolCallId: workflowRef,
      commandSessionId: normalizedSessionId,
      ...buildToolIdentityMeta(source, {
        tool_display_name: executeCommandToolName
      }),
      modelRound: Number.isFinite(lastRound) ? lastRound : undefined,
      ...usageMeta,
      ...buildWorkflowTimingMeta(source)
    });
    assistantMessage.workflowItems.push(item);
    registerToolItem(executeCommandToolName, item.id, workflowRef);
    toolCallItemMap.set(normalizedSessionId, item.id);
    return item.id;
  };

  const mergeCommandSessionSummaryIntoBuffer = (buffer, source) => {
    if (!buffer || !source || typeof source !== 'object') return;
    const stdoutTail = pickString(
      source.stdout_tail,
      source.stdoutTail,
      source.pty_tail,
      source.ptyTail
    );
    const stderrTail = pickString(source.stderr_tail, source.stderrTail);
    const command = pickString(source.command, buffer.command);
    if (command && !buffer.command) {
      buffer.command = command;
    }
    if (stdoutTail && !buffer.stdout) {
      buffer.stdout = stdoutTail;
    }
    if (stderrTail && !buffer.stderr) {
      buffer.stderr = stderrTail;
    }
    const stdoutDropped = toOptionalInt(
      source.stdout_dropped_bytes,
      source.stdoutDroppedBytes,
      source.pty_dropped_bytes,
      source.ptyDroppedBytes
    );
    const stderrDropped = toOptionalInt(
      source.stderr_dropped_bytes,
      source.stderrDroppedBytes
    );
    if (stdoutDropped !== null && stdoutDropped > 0) {
      buffer.stdoutDropped = Math.max(Number(buffer.stdoutDropped) || 0, stdoutDropped);
    }
    if (stderrDropped !== null && stderrDropped > 0) {
      buffer.stderrDropped = Math.max(Number(buffer.stderrDropped) || 0, stderrDropped);
    }
  };

  const upsertCommandSessionResultItem = (commandSessionId, source, failed) => {
    const normalizedSessionId = normalizeCommandSessionRef(commandSessionId);
    if (!normalizedSessionId) return null;
    const normalizedToolCallId = normalizeToolCallRef(
      source?.tool_call_id ?? source?.toolCallId ?? source?.call_id ?? source?.callId
    );
    const workflowRef = normalizedToolCallId || normalizedSessionId;
    const toolCategory = resolveToolCategory(executeCommandToolName, source);
    const commandIndex = toOptionalInt(source?.command_index, source?.commandIndex);
    const title = buildCommandSessionTitle(source?.command, commandIndex);
    const exitCode = toOptionalInt(
      source?.returncode,
      source?.exit_code,
      source?.exitCode
    );
    const detail = buildDetail({
      ...(source && typeof source === 'object' ? source : {}),
      tool: executeCommandToolName,
      command_session_id: normalizedSessionId,
      command: pickString(source?.command),
      returncode: exitCode,
      exit_code: exitCode,
      stdout: pickString(
        source?.stdout,
        source?.stdout_tail,
        source?.stdoutTail,
        source?.pty_tail,
        source?.ptyTail
      ),
      stderr: pickString(source?.stderr, source?.stderr_tail, source?.stderrTail)
    });
    const status = failed ? 'failed' : 'completed';
    const patch = {
      title,
      detail,
      status,
      isTool: true,
      toolCategory,
      eventType: 'tool_result',
      toolName: executeCommandToolName,
      toolCallId: workflowRef,
      commandSessionId: normalizedSessionId,
      ...buildToolIdentityMeta(source, {
        tool_display_name: executeCommandToolName
      }),
      ...buildWorkflowUsageMeta(source),
      ...buildWorkflowTimingMeta(source)
    };
    const existing =
      commandSessionResultItemMap.get(workflowRef)
      || commandSessionResultItemMap.get(normalizedSessionId);
    if (existing) {
      updateWorkflowItem(assistantMessage.workflowItems, existing, patch);
      commandSessionResultItemMap.set(normalizedSessionId, existing);
      if (normalizedToolCallId) {
        commandSessionResultItemMap.set(normalizedToolCallId, existing);
      }
      return existing;
    }
    const item = buildWorkflowItem(title, detail, status, {
      isTool: true,
      toolCategory,
      eventType: 'tool_result',
      toolName: executeCommandToolName,
      toolCallId: workflowRef,
      commandSessionId: normalizedSessionId,
      ...buildToolIdentityMeta(source, {
        tool_display_name: executeCommandToolName
      }),
      ...buildWorkflowUsageMeta(source),
      ...buildWorkflowTimingMeta(source)
    });
    assistantMessage.workflowItems.push(item);
    commandSessionResultItemMap.set(normalizedSessionId, item.id);
    if (normalizedToolCallId) {
      commandSessionResultItemMap.set(normalizedToolCallId, item.id);
    }
    return item.id;
  };

  const extractCommandSessionResultRows = (source) => {
    if (!source || typeof source !== 'object') return [];
    const dataObject =
      source.data && typeof source.data === 'object'
        ? source.data
        : source;
    if (!Array.isArray(dataObject?.results)) {
      return [];
    }
    return dataObject.results
      .map((item) => (item && typeof item === 'object' ? item : null))
      .filter((item) => normalizeCommandSessionRef(item?.command_session_id ?? item?.commandSessionId))
      .map((item) => ({
        commandSessionId: normalizeCommandSessionRef(
          item?.command_session_id ?? item?.commandSessionId
        ),
        source: item
      }));
  };

  const isContextOverflowText = (value) => {
    const normalized = String(value || '').trim().toLowerCase();
    if (!normalized) return false;
    return [
      'context_window_exceeded',
      'context length exceeded',
      'context window',
      'input exceeds the context window',
      'exceeds the model',
      'prompt is too long',
      '上下文',
      '超限',
      '过长'
    ].some((token) => normalized.includes(token));
  };

  const resolveLatestCompactionRef = () => {
    if (activeCompactionWorkflowRef) {
      return activeCompactionWorkflowRef;
    }
    const refs = Array.from(compactionProgressItemMap.keys());
    return refs.length > 0 ? refs[refs.length - 1] : null;
  };

  const resolveCompactionInstanceRef = (detailPayload, fallbackRound = null) => {
    const payload =
      detailPayload && typeof detailPayload === 'object'
        ? detailPayload
        : {};
    const compactionId = pickString(
      payload?.compaction_id,
      payload?.compactionId
    );
    if (compactionId) {
      return `compaction-id:${compactionId}`;
    }
    const triggerMode = pickString(payload?.trigger_mode, payload?.triggerMode) || 'auto_loop';
    compactionAnonymousRefSeq += 1;
    if (Number.isFinite(fallbackRound)) {
      return `compaction:${triggerMode}:${fallbackRound}:${compactionAnonymousRefSeq}`;
    }
    return `compaction:${triggerMode}:${compactionAnonymousRefSeq}`;
  };

  const markCompactionProgressFailed = (detailPayload) => {
    const workflowRef = resolveLatestCompactionRef();
    if (!workflowRef) return false;
    const itemId = compactionProgressItemMap.get(workflowRef);
    if (!itemId) return false;
    const existingItem = assistantMessage.workflowItems.find((item) => item.id === itemId) || null;
    const existingDetail = safeJsonParse(existingItem?.detail);
    const mergedDetail = {
      ...(existingDetail && typeof existingDetail === 'object' ? existingDetail : {}),
      ...(detailPayload && typeof detailPayload === 'object' ? detailPayload : {}),
      status: 'failed',
      stage:
        detailPayload?.stage
        ?? existingDetail?.stage
        ?? 'context_overflow_recovery'
    };
    updateWorkflowItem(assistantMessage.workflowItems, itemId, {
      status: 'failed',
      detail: buildDetail(mergedDetail)
    });
    clearCompactionProgressRef(workflowRef);
    return true;
  };

  const finalizeCompactionProgressItem = (workflowRef, detailPayload, status) => {
    if (!workflowRef) return false;
    const itemId = compactionProgressItemMap.get(workflowRef);
    if (!itemId) return false;
    const existingItem = assistantMessage.workflowItems.find((item) => item.id === itemId) || null;
    const existingDetail = safeJsonParse(existingItem?.detail);
    const mergedDetail = {
      ...(existingDetail && typeof existingDetail === 'object' ? existingDetail : {}),
      ...(detailPayload && typeof detailPayload === 'object' ? detailPayload : {}),
      status:
        detailPayload?.status
        ?? (status === 'failed' ? 'failed' : 'done')
    };
    updateWorkflowItem(assistantMessage.workflowItems, itemId, {
      title: t('chat.toolWorkflow.compaction.title'),
      status,
      detail: buildDetail(mergedDetail),
      isTool: true,
      eventType: 'compaction',
      toolName: '上下文压缩',
      toolCallId: workflowRef
    });
    appendCompactionOutcomeNotice(workflowRef, mergedDetail, status);
    clearCompactionProgressRef(workflowRef);
    return true;
  };

  const resolveCompactionOutcomeNotice = (detailPayload, status) => {
    const normalizedStatus = String(status || detailPayload?.status || '').trim().toLowerCase();
    if (normalizedStatus === 'failed' || normalizedStatus === 'error') {
      return null;
    }
    const currentInputTrimmed = Boolean(
      detailPayload?.context_guard_current_user_trimmed ?? detailPayload?.current_user_trimmed
    );
    if (currentInputTrimmed) {
      return {
        kind: 'current_input_trimmed',
        title: t('chat.toolWorkflow.compaction.noticeInputTrimmedTitle'),
        detail: t('chat.toolWorkflow.compaction.noticeInputTrimmedDetail')
      };
    }
    const historyCompressed = Boolean(
      detailPayload?.context_guard_summary_removed
      ?? detailPayload?.context_guard_summary_trimmed
      ?? detailPayload?.summary_removed
      ?? detailPayload?.summary_trimmed
    );
    if (historyCompressed) {
      return {
        kind: 'history_compacted',
        title: t('chat.toolWorkflow.compaction.noticeHistoryCompactedTitle'),
        detail: t('chat.toolWorkflow.compaction.noticeHistoryCompactedDetail')
      };
    }
    return null;
  };

  const appendCompactionOutcomeNotice = (workflowRef, detailPayload, status) => {
    const notice = resolveCompactionOutcomeNotice(detailPayload, status);
    if (!notice) return;
    const existing = assistantMessage.workflowItems.some((item) => {
      const eventType = String(item?.eventType || '').trim().toLowerCase();
      return (
        eventType === 'compaction_notice'
        && String(item?.toolCallId || '') === String(workflowRef || '')
        && String(item?.noticeKind || '') === notice.kind
      );
    });
    if (existing) {
      return;
    }
    assistantMessage.workflowItems.push(
      buildWorkflowItem(notice.title, notice.detail, 'completed', {
        eventType: 'compaction_notice',
        toolCallId: workflowRef || undefined,
        noticeKind: notice.kind
      })
    );
  };

  const isPendingCompactionStatus = (value) => {
    const normalized = String(value || '').trim().toLowerCase();
    return (
      !normalized
      || normalized === 'loading'
      || normalized === 'pending'
      || normalized === 'running'
      || normalized === 'in_progress'
    );
  };

  const resolveLingeringCompactionProgressItemIds = () => {
    const ids = new Set<string>();
    compactionProgressItemMap.forEach((itemId) => {
      const normalized = String(itemId || '').trim();
      if (normalized) {
        ids.add(normalized);
      }
    });
    assistantMessage.workflowItems.forEach((item) => {
      const eventType = String(item?.eventType || item?.event || '').trim().toLowerCase();
      if (eventType !== 'compaction_progress') return;
      if (!isPendingCompactionStatus(item?.status)) return;
      const itemId = String(item?.id || '').trim();
      if (itemId) {
        ids.add(itemId);
      }
    });
    return Array.from(ids);
  };

  const finalizeLingeringCompactionProgressItems = (detailPayload, status) => {
    const entries = Array.from(compactionProgressItemMap.entries());
    const itemIds = resolveLingeringCompactionProgressItemIds();
    if (!itemIds.length) {
      if (compactionProgressItemMap.size > 0 || activeCompactionWorkflowRef) {
        compactionProgressItemMap.clear();
        activeCompactionWorkflowRef = null;
      }
      return false;
    }
    const normalizedDetailPayload =
      detailPayload && typeof detailPayload === 'object'
        ? (detailPayload as Record<string, unknown>)
        : {};
    let finalized = false;
    // Reconcile orphaned compaction progress entries when compaction/final events arrive out of order.
    itemIds.forEach((itemId) => {
      const existingItem = assistantMessage.workflowItems.find((item) => item.id === itemId) || null;
      if (!existingItem || !isPendingCompactionStatus(existingItem.status)) {
        return;
      }
      const workflowRef =
        entries.find((entry) => String(entry[1] || '').trim() === String(itemId || '').trim())?.[0]
        || activeCompactionWorkflowRef;
      const existingDetail = safeJsonParse(existingItem.detail);
      const mergedDetail = {
        ...(existingDetail && typeof existingDetail === 'object' ? existingDetail : {}),
        ...normalizedDetailPayload,
        status:
          normalizedDetailPayload?.status
          ?? (status === 'failed' ? 'failed' : 'done')
      };
      updateWorkflowItem(assistantMessage.workflowItems, itemId, {
        title: t('chat.toolWorkflow.compaction.title'),
        status,
        detail: buildDetail(mergedDetail),
        isTool: true,
        eventType: 'compaction',
        toolCallId: workflowRef || undefined
      });
      appendCompactionOutcomeNotice(workflowRef, mergedDetail, status);
      finalized = true;
    });
    compactionProgressItemMap.clear();
    activeCompactionWorkflowRef = null;
    return finalized;
  };

  const isCompactionWorkflowEventItem = (item) => {
    const eventType = String(item?.eventType || item?.event || '').trim().toLowerCase();
    return eventType === 'compaction' || eventType === 'compaction_progress' || eventType === 'compaction_notice';
  };

  const shouldKeepCompactionMarkerLayout = () => {
    if (String(assistantMessage.content || '').trim()) return false;
    if (String(resolveReasoningOutput() || '').trim()) return false;
    if (hasPlanSteps(assistantMessage.plan)) return false;
    const panelStatus = String(assistantMessage?.questionPanel?.status || '').trim().toLowerCase();
    if (panelStatus === 'pending') return false;
    if (!Array.isArray(assistantMessage.workflowItems) || assistantMessage.workflowItems.length === 0) {
      return false;
    }
    return assistantMessage.workflowItems.every((item) => isCompactionWorkflowEventItem(item));
  };

  const resolveCompactionFallbackStatus = (): 'completed' | 'failed' => {
    if (compactionTerminalStatusHint) {
      return compactionTerminalStatusHint;
    }
    for (let cursor = assistantMessage.workflowItems.length - 1; cursor >= 0; cursor -= 1) {
      const item = assistantMessage.workflowItems[cursor];
      const status = String(item?.status || '').trim().toLowerCase();
      if (status === 'failed') {
        return 'failed';
      }
      const eventType = String(item?.eventType || item?.event || '').trim().toLowerCase();
      if (eventType === 'error' || eventType === 'request_failed' || eventType === 'team_error') {
        return 'failed';
      }
    }
    return 'completed';
  };

  const allocateCompactionWorkflowRef = (round) => {
    compactionAnonymousRefSeq += 1;
    if (Number.isFinite(round)) {
      return `compaction:${round}:${compactionAnonymousRefSeq}`;
    }
    return `compaction:auto:${compactionAnonymousRefSeq}`;
  };

  const resolveExistingCompactionWorkflowRef = () => {
    const items = Array.isArray(assistantMessage?.workflowItems) ? assistantMessage.workflowItems : [];
    for (let cursor = items.length - 1; cursor >= 0; cursor -= 1) {
      const ref = String(items[cursor]?.toolCallId || items[cursor]?.tool_call_id || '').trim();
      if (ref.startsWith('compaction:')) {
        return ref;
      }
    }
    return '';
  };

  const resolveActiveCompactionWorkflowRef = (round, detailPayload = null) => {
    const instanceRef = resolveCompactionInstanceRef(detailPayload, round);
    if (instanceRef) {
      activeCompactionWorkflowRef = instanceRef;
      return instanceRef;
    }
    if (!activeCompactionWorkflowRef) {
      activeCompactionWorkflowRef =
        resolveExistingCompactionWorkflowRef() || allocateCompactionWorkflowRef(round);
    }
    return activeCompactionWorkflowRef;
  };

  const resolveStandaloneCompactionWorkflowRef = (round, detailPayload = null) =>
    resolveCompactionInstanceRef(detailPayload, round) ||
    activeCompactionWorkflowRef ||
    resolveExistingCompactionWorkflowRef() ||
    allocateCompactionWorkflowRef(round);

  const ensureCompactionProgressItem = (title, detail, workflowRef) => {
    if (!workflowRef) return null;
    const existing = compactionProgressItemMap.get(workflowRef);
    if (existing) {
      updateWorkflowItem(assistantMessage.workflowItems, existing, {
        title,
        detail,
        status: 'loading',
        isTool: true,
        eventType: 'compaction_progress',
        toolName: '上下文压缩',
        toolCallId: workflowRef
      });
      return existing;
    }
    // Keep compaction progress and final result under the same workflow ref,
    // so the UI can render them as a single evolving timeline entry.
    const item = buildWorkflowItem(title, detail, 'loading', {
      isTool: true,
      eventType: 'compaction_progress',
      toolName: '上下文压缩',
      toolCallId: workflowRef
    });
    assistantMessage.workflowItems.push(item);
    compactionProgressItemMap.set(workflowRef, item.id);
    activeCompactionWorkflowRef = workflowRef;
    return item.id;
  };

  const clearCompactionProgressRef = (workflowRef) => {
    if (workflowRef) {
      compactionProgressItemMap.delete(workflowRef);
    }
    if (!workflowRef || activeCompactionWorkflowRef === workflowRef) {
      activeCompactionWorkflowRef = null;
    }
  };

  const normalizeSubagentWorkflowStatus = (value) => {
    const normalized = String(value || '').trim().toLowerCase();
    if (
      normalized === 'error' ||
      normalized === 'failed' ||
      normalized === 'timeout' ||
      normalized === 'cancelled' ||
      normalized === 'closed' ||
      normalized === 'partial' ||
      normalized === 'not_found'
    ) {
      return 'failed';
    }
    if (
      normalized === 'running' ||
      normalized === 'waiting' ||
      normalized === 'queued' ||
      normalized === 'accepted' ||
      normalized === 'cancelling'
    ) {
      return 'loading';
    }
    return 'completed';
  };

  const ensureSubagentDispatchItem = (dispatchId, title, detail, status = 'loading') => {
    const key = String(dispatchId || '').trim();
    if (!key) return null;
    const payload = safeJsonParse(detail);
    subagentDispatchItemMap.set(key, {
      title,
      status,
      detail: payload && typeof payload === 'object' ? payload : detail
    });
    return key;
  };

  const upsertSubagentRunItem = (runKey, title, detail, status, meta: Record<string, unknown> = {}) => {
    const key = String(runKey || '').trim();
    const source =
      meta.source && typeof meta.source === 'object'
        ? meta.source
        : safeJsonParse(detail);
    const payload =
      source && typeof source === 'object'
        ? {
            ...source,
            title,
            label: source.label ?? source.spawn_label ?? source.title ?? title,
            status: source.status ?? status
          }
        : {
            run_id: key,
            title,
            label: title,
            status,
            summary: detail
          };
    if (!key && !payload.session_id && !payload.run_id) {
      return;
    }
    const normalized = upsertMessageSubagent(assistantMessage, payload);
    if (normalized) {
      subagentRunItemMap.set(normalized.key, normalized.key);
    }
  };

  const upsertSubagentPayloads = (source, fallbackTitle, fallbackStatus, meta: Record<string, unknown> = {}) => {
    const payloads = collectSubagentPayloads(source);
    if (!payloads.length) {
      const record = source && typeof source === 'object' ? (source as Record<string, unknown>) : {};
      upsertSubagentRunItem(
        record.run_id ?? record.runId ?? record.session_id ?? record.sessionId ?? '',
        fallbackTitle,
        buildDetail(record),
        fallbackStatus,
        { ...meta, source: record }
      );
      return;
    }
    payloads.forEach((item) => {
      const label = String(item.label ?? item.spawn_label ?? item.spawnLabel ?? item.title ?? '').trim();
      const sessionId = String(item.session_id ?? item.sessionId ?? '').trim();
      const runId = String(item.run_id ?? item.runId ?? '').trim();
      const itemTitle = label || sessionId || runId || fallbackTitle;
      const itemStatus = normalizeSubagentEventStatus(item.status ?? fallbackStatus);
      upsertSubagentRunItem(
        runId || sessionId,
        itemTitle,
        buildDetail(item),
        itemStatus,
        { ...meta, source: item }
      );
    });
  };

  const updateRoundState = (roundNumber) => {
    if (!Number.isFinite(roundNumber)) {
      return;
    }
    if (!Number.isFinite(roundState.globalRound) || roundState.globalRound < roundNumber) {
      roundState.globalRound = roundNumber;
    }
    roundState.currentRound = roundNumber;
  };

  const resolveModelRound = (payload, data) => {
    const directRound = normalizeStreamRound(
      data?.model_round ??
        payload?.model_round ??
        data?.round ??
        payload?.round
    );
    if (directRound !== null) {
      return directRound;
    }
    for (const source of [data, payload]) {
      const segments = readDeltaSegments(source);
      if (!segments.length) {
        continue;
      }
      let segmentRound = null;
      segments.forEach((segment) => {
        if (!segment || typeof segment !== 'object') return;
        const resolved = normalizeStreamRound(segment.model_round ?? segment.round);
        if (resolved !== null) {
          segmentRound = resolved;
        }
      });
      if (segmentRound !== null) {
        return segmentRound;
      }
    }
    return null;
  };

  const resolveRound = (payload, data) => {
    const roundNumber = resolveModelRound(payload, data);
    if (roundNumber !== null) {
      updateRoundState(roundNumber);
      return roundNumber;
    }
    return Number.isFinite(roundState.currentRound) ? roundState.currentRound : null;
  };

  const advanceModelRound = () => {
    const nextRound = (Number.isFinite(roundState.globalRound) ? roundState.globalRound : 0) + 1;
    updateRoundState(nextRound);
    return nextRound;
  };

  const buildOutputDetail = () => {
    const parts = [];
    const reasoningText = resolveReasoningOutput();
    if (reasoningText) {
      parts.push(`[${t('chat.workflow.detail.reasoning')}]\n${tailText(reasoningText)}`);
    }
    if (outputContent) {
      parts.push(`[${t('chat.workflow.detail.output')}]\n${tailText(outputContent)}`);
    }
    if (!parts.length) {
      return tailText(assistantMessage.content || '');
    }
    return parts.join('\n\n');
  };

  let pendingContent = '';
  let pendingReasoningExplicit = '';
  let pendingReasoningFallback = '';
  const thinkStreamParser = createThinkTagStreamParser();
  let streamTimer = null;
  const flushStream = (force = false) => {
    if (streamTimer !== null) {
      clearTimeout(streamTimer);
      streamTimer = null;
    }
    if (force) {
      const trailingDelta = thinkStreamParser.push('', true);
      if (trailingDelta.content) {
        pendingContent += trailingDelta.content;
      }
      if (trailingDelta.reasoning) {
        pendingReasoningFallback += trailingDelta.reasoning;
      }
    }
    const hasContentDelta = Boolean(pendingContent);
    const hasReasoningDelta = Boolean(
      pendingReasoningExplicit || pendingReasoningFallback
    );
    if (!hasContentDelta && !hasReasoningDelta && !force) {
      return;
    }
    if (pendingReasoningExplicit) {
      outputReasoningExplicit += pendingReasoningExplicit;
      pendingReasoningExplicit = '';
      outputState.reasoningStreaming = true;
    }
    if (pendingReasoningFallback) {
      outputReasoningFallback += pendingReasoningFallback;
      pendingReasoningFallback = '';
      outputState.reasoningStreaming = true;
    }
    if (hasContentDelta) {
      outputContent += pendingContent;
      pendingContent = '';
      assistantMessage.content = outputContent;
      outputState.streaming = true;
    }
    if (hasContentDelta && stats) {
      const baseTokens =
        normalizeContextTokens(contextEstimateBaseTokens) ??
        normalizeContextTokens(stats.contextTokens) ??
        resolveContextPreviewTokens(stats);
      if (baseTokens !== null && baseTokens > 0) {
        const estimatedOutputTokens = estimateStreamOutputTokens(outputContent);
        if (estimatedOutputTokens > 0) {
          const estimatedTotal = baseTokens + estimatedOutputTokens;
          const currentTokens =
            normalizeContextTokens(stats.contextTokens) ??
            resolveContextPreviewTokens(stats);
          if (currentTokens === null || estimatedTotal > currentTokens) {
            syncContextPreviewTokens(estimatedTotal);
          }
        }
      }
    }
    syncReasoningToMessage();
    if (hasContentDelta || hasReasoningDelta) {
      markAssistantWaitingOutputVisible(assistantMessage);
      const outputId = ensureOutputItem();
      updateWorkflowItem(assistantMessage.workflowItems, outputId, {
        detail: buildOutputDetail()
      });
    }
    if (hasContentDelta || hasReasoningDelta) {
      notifySnapshot();
    }
  };

  const scheduleStreamFlush = () => {
    flushStream();
  };

  const resetStreamPending = () => {
    if (streamTimer !== null) {
      clearTimeout(streamTimer);
      streamTimer = null;
    }
    pendingContent = '';
    pendingReasoningExplicit = '';
    pendingReasoningFallback = '';
    thinkStreamParser.reset();
  };

  const notifySnapshot = (immediate = false) => {
    if (typeof onSnapshot === 'function') {
      onSnapshot(immediate);
    }
  };

  ensureInteractionStart();

  const clearVisibleOutput = () => {
    resetStreamPending();
    assistantMessage.content = '';
    outputContent = '';
    outputReasoningExplicit = '';
    outputReasoningFallback = '';
    outputState.streaming = false;
    outputState.reasoningStreaming = false;
    assistantMessage.stream_round = null;
    syncReasoningToMessage();
    if (outputItemId) {
      updateWorkflowItem(assistantMessage.workflowItems, outputItemId, {
        detail: '',
        status: 'loading'
      });
    }
    visibleRound = null;
  };

  // 续传时需要把已有的工具调用记录挂载到映射，避免结果无法回填
  if (Array.isArray(assistantMessage.workflowItems)) {
    assistantMessage.workflowItems.forEach((item) => {
      const toolName = resolveWorkflowItemToolName(item);
      const itemStatus = String(item?.status || '').trim().toLowerCase();
      const eventType = String(item?.eventType || item?.event || '').trim().toLowerCase();
      const toolCallId = normalizeToolCallRef(item?.toolCallId);
      if (toolName && (itemStatus === 'loading' || itemStatus === 'pending') && eventType === 'tool_call') {
        registerToolItem(toolName, item.id, item?.toolCallId);
      }
      if (toolName && (itemStatus === 'loading' || itemStatus === 'pending') && eventType === 'tool_output_delta') {
        const outputKey = resolveToolOutputKey(toolName, toolCallId, item?.commandSessionId);
        toolOutputItemMap.set(outputKey, item.id);
        const buffer = {
          command: extractToolOutputSection(item?.detail, 'command'),
          stdout: extractToolOutputSection(item?.detail, 'stdout'),
          stderr: extractToolOutputSection(item?.detail, 'stderr'),
          stdoutDropped: 0,
          stderrDropped: 0
        };
        if (buffer.command || buffer.stdout || buffer.stderr) {
          toolOutputBufferMap.set(outputKey, buffer);
        }
      }
      if (toolCallId && eventType === 'tool_result' && isExecuteCommandTool(toolName)) {
        commandSessionResultItemMap.set(toolCallId, item.id);
      }
      const approvalId = String(item?.approvalId || '').trim();
      if (approvalId && eventType === 'approval_request' && (itemStatus === 'loading' || itemStatus === 'pending')) {
        approvalItemMap.set(approvalId, item.id);
      }
    });
  }
  if (initialRound !== null) {
    updateRoundState(initialRound);
  }

  const ensureOutputItem = () => {
    if (!outputItemId) {
      const item = buildWorkflowItem('模型输出', '', 'loading', {
        eventType: 'llm_output',
        isModelOutput: true
      });
      outputItemId = item.id;
      assistantMessage.workflowItems.push(item);
    }
    return outputItemId;
  };

  const applyQuestionPanelPayload = (payload, options: QuestionPanelApplyOptions = {}) => {
    const normalized = normalizeInquiryPanelPayload(payload);
    if (!normalized) return false;
    assistantMessage.questionPanel = {
      ...normalized,
      status: 'pending',
      selected: []
    };
    if (options.appendWorkflow !== false) {
      assistantMessage.workflowItems.push(
        buildWorkflowItem('问询面板', buildDetail(normalized))
      );
    }
    return true;
  };

  const eventContext = {
    perfEnabled, processorSessionId, approvalItemMap, compactionProgressItemMap, modelRoundUsagePayloadMap,
    blockedRounds, outputState, finalizeWithNow, stats, applyInteractionTimestamp, resolveReasoningOutput,
    syncReasoningToMessage, executeCommandToolName, isExecuteCommandTool, isSubagentControlTool,
    extractCommandSessionRef, syncCommandSessionSnapshot, syncCommandSessionDelta, buildCommandSessionTitle,
    normalizeStopReason, isToolFailureGuardStopReason, markManualCompactionMarker,
    appendToolFailureGuardWorkflowItem, registerToolStats, updateBackendTurnSpeedStats,
    updatePartialConsumedFromUsage, updateUsageStats, updateLiveContextUsageFromTokenUsage,
    updateLiveContextUsageFromRequest, updateRoundUsageStats, updateQuotaUsage, fallbackQuotaUsageFromRound,
    updateContextUsage, extractToolCallRef, registerToolItem, applyModelRoundUsageToWorkflowTools,
    resolveToolItemId, peekToolItemId, resolveToolOutputKey, getToolOutputBuffer, appendToolOutput,
    buildToolOutputDetail, clearToolOutputFlush, scheduleToolOutputFlush, ensureToolOutputItem,
    finalizeToolOutputItem, resolveCommandSessionStatus, ensureCommandSessionCallItem,
    mergeCommandSessionSummaryIntoBuffer, upsertCommandSessionResultItem, extractCommandSessionResultRows,
    isContextOverflowText, markCompactionProgressFailed, finalizeCompactionProgressItem,
    appendCompactionOutcomeNotice, finalizeLingeringCompactionProgressItems, shouldKeepCompactionMarkerLayout, resolveActiveCompactionWorkflowRef, resolveStandaloneCompactionWorkflowRef, ensureCompactionProgressItem,
    clearCompactionProgressRef, normalizeSubagentWorkflowStatus, ensureSubagentDispatchItem, upsertSubagentPayloads, resolveRound, advanceModelRound, buildOutputDetail, thinkStreamParser,
    flushStream, scheduleStreamFlush, notifySnapshot, clearVisibleOutput, ensureOutputItem,
    applyQuestionPanelPayload, assistantMessage, options,
    get outputItemId() { return outputItemId; }, get toolFailureGuardNotified() { return toolFailureGuardNotified; },
    set toolFailureGuardNotified(value) { toolFailureGuardNotified = value; }, get lastRound() { return lastRound; },
    set lastRound(value) { lastRound = value; }, get activeCompactionWorkflowRef() { return activeCompactionWorkflowRef; },
    set activeCompactionWorkflowRef(value) { activeCompactionWorkflowRef = value; }, get compactionTerminalStatusHint() { return compactionTerminalStatusHint; },
    set compactionTerminalStatusHint(value) { compactionTerminalStatusHint = value; }, get visibleRound() { return visibleRound; },
    set visibleRound(value) { visibleRound = value; }, get outputContent() { return outputContent; },
    set outputContent(value) { outputContent = value; }, get outputReasoningExplicit() { return outputReasoningExplicit; }, set outputReasoningExplicit(value) { outputReasoningExplicit = value; },
    get outputReasoningFallback() { return outputReasoningFallback; }, set outputReasoningFallback(value) { outputReasoningFallback = value; }, get pendingContent() { return pendingContent; }, set pendingContent(value) { pendingContent = value; },
    get pendingReasoningExplicit() { return pendingReasoningExplicit; }, set pendingReasoningExplicit(value) { pendingReasoningExplicit = value; }, get pendingReasoningFallback() { return pendingReasoningFallback; }, set pendingReasoningFallback(value) { pendingReasoningFallback = value; }
  };
  const handleEvent = (eventName, raw) => handleWorkflowProcessorEvent(eventContext, eventName, raw);

  const finalize = () => {
    flushStream(true);
    flushAllToolOutputs();
    outputState.streaming = false;
    outputState.reasoningStreaming = false;
    syncReasoningToMessage();
    if (
      !normalizeFlag(assistantMessage.workflowStreaming)
      && !normalizeFlag(assistantMessage.stream_incomplete)
      && !normalizeFlag(assistantMessage.reasoningStreaming)
    ) {
      finalizeLingeringCompactionProgressItems({}, resolveCompactionFallbackStatus());
    }
    if (outputItemId) {
      updateWorkflowItem(assistantMessage.workflowItems, outputItemId, {
        status: 'completed'
      });
    }
    finalizeInteractionDuration();
    notifySnapshot();
  };

  return { handleEvent, finalize };
};
