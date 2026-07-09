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

import { safeJsonParse, stringifyPayload } from './chatDemoPanels';
import { syncSessionPendingApprovalRuntime } from './chatRuntimeState';
import { buildMessage, resolveTimestampIso, resolveTimestampMs } from './chatStats';
import { SessionWorkflowStateOptions } from './chatTypes';

export const sessionWorkflowState = new Map();

export const buildSessionWorkflowState = () => ({
  globalRound: 0,
  currentRound: null
});

export const normalizeSessionWorkflowState = (state) => {
  if (!state || typeof state !== 'object') {
    return buildSessionWorkflowState();
  }
  if (!Number.isFinite(state.globalRound)) {
    state.globalRound = 0;
  }
  if (!Number.isFinite(state.currentRound)) {
    state.currentRound = null;
  }
  return state;
};

export const getSessionWorkflowState = (sessionId, options: SessionWorkflowStateOptions = {}) => {
  const sessionKey = sessionId ? String(sessionId) : '';
  if (!sessionKey) {
    return buildSessionWorkflowState();
  }
  const reset = options.reset === true;
  let state = sessionWorkflowState.get(sessionKey);
  if (!state || reset) {
    state = buildSessionWorkflowState();
    sessionWorkflowState.set(sessionKey, state);
  }
  return normalizeSessionWorkflowState(state);
};

export const updateWorkflowItem = (items, id, patch) => {
  const target = items.find((item) => item.id === id);
  if (target) {
    Object.assign(target, patch);
  }
};

export const resolveEventType = (eventName, payload) => {
  // SSE 浜嬩欢鍚嶄紭鍏堬紝浣嗛亣鍒伴粯璁ょ殑 message 鏃跺厑璁镐娇鐢?payload 鍐呴儴瀛楁
  const normalized = (eventName || '').trim();
  if (normalized && normalized !== 'message') return normalized;
  if (payload?.event) return payload.event;
  if (payload?.type) return payload.type;
  return normalized || 'message';
};

export const normalizeStreamEventType = (eventType) => String(eventType || '').trim().toLowerCase();

export const resolveNormalizedStreamEventType = (eventName, payload) =>
  normalizeStreamEventType(resolveEventType(eventName, payload));

export const isTerminalStreamEventType = (eventType) => {
  const normalized = normalizeStreamEventType(eventType);
  return (
    normalized === 'final' ||
    normalized === 'error' ||
    normalized === 'queue_finish' ||
    normalized === 'queue_fail' ||
    normalized === 'turn_terminal'
  );
};

export const isTerminalRuntimeStatus = (status) => {
  const normalizedStatus = normalizeThreadRuntimeStatus(status);
  return normalizedStatus !== 'running' &&
    normalizedStatus !== 'queued' &&
    !isThreadRuntimeWaiting(normalizedStatus);
};

export const shouldTreatRuntimeEventAsTerminal = (eventType, payload) => {
  const normalizedEventType = normalizeStreamEventType(eventType);
  if (normalizedEventType === 'thread_closed') {
    return true;
  }
  if (normalizedEventType !== 'thread_status') {
    return false;
  }
  return isTerminalRuntimeStatus(payload?.thread_status ?? payload?.status);
};

export const isTerminalLlmOutputPayload = (payload, data = null) => {
  const source = data && typeof data === 'object' ? data : {};
  const stopReason = String(
    source?.stop_reason ??
      source?.stopReason ??
      source?.finish_reason ??
      source?.finishReason ??
      payload?.stop_reason ??
      payload?.stopReason ??
      payload?.finish_reason ??
      payload?.finishReason ??
      ''
  ).trim();
  if (stopReason) {
    return true;
  }
  const terminalFlags = [
    source?.done,
    source?.is_final,
    source?.isFinal,
    source?.final,
    payload?.done,
    payload?.is_final,
    payload?.isFinal,
    payload?.final
  ];
  return terminalFlags.some((flag) => flag === true);
};

export const handleApprovalEvent = (store, eventType, payload, requestId, sessionId) => {
  if (!store) return;
  if (eventType === 'approval_request') {
    store.enqueueApprovalRequest(requestId, sessionId, payload);
    syncSessionPendingApprovalRuntime(store, sessionId);
    return;
  }
  if (eventType === 'approval_result') {
    store.resolveApprovalResult(payload);
    syncSessionPendingApprovalRuntime(store, sessionId);
  }
};

export const pickText = (value, fallback = '') => {
  if (value === null || value === undefined) return fallback;
  if (typeof value === 'string') return value;
  return stringifyPayload(value);
};

// 淇濈暀瀹屾暣璇︽儏锛屼緵寮圭獥鏌ョ湅瀹屾暣鍐呭
export const pickString = (...values) => {
  for (const value of values) {
    if (typeof value === 'string') {
      const normalized = value.trim();
      if (normalized) return value;
      continue;
    }
    if (typeof value === 'number' && Number.isFinite(value)) {
      return String(value);
    }
  }
  return '';
};

export const toOptionalInt = (...values) => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return Math.trunc(value);
    }
    if (typeof value === 'string') {
      const normalized = value.trim();
      if (!normalized) continue;
      const parsed = Number(normalized);
      if (Number.isFinite(parsed)) {
        return Math.trunc(parsed);
      }
    }
  }
  return null;
};

export const buildDetail = (payload) => stringifyPayload(payload);

export const defaultSessionTitles = new Set(['\u65b0\u4f1a\u8bdd', '\u672a\u547d\u540d\u4f1a\u8bdd']);

export const buildSessionTitle = (content, maxLength = 20) => {
  const cleaned = String(content || '').trim().replace(/\s+/g, ' ');
  if (!cleaned) return '';
  if (cleaned.length <= maxLength) return cleaned;
  return `${cleaned.slice(0, maxLength)}...`;
};

export const shouldAutoTitle = (title) => {
  if (!title) return true;
  return defaultSessionTitles.has(String(title).trim());
};

export const extractAnswerFromPayload = (payload) => {
  if (!payload || typeof payload !== 'object') return '';
  const data = payload.data;
  if (data && typeof data === 'object') {
    const answer = data.answer || data.content || data.message;
    if (answer) return String(answer);
  }
  const answer = payload.answer || payload.content || payload.message;
  return answer ? String(answer) : '';
};

export const THINK_OPEN_TAG = '<think>';
export const THINK_CLOSE_TAG = '</think>';

export const normalizeReasoningText = (value) =>
  typeof value === 'string' ? value : value ? String(value) : '';

export const resolveAssistantReasoning = (payload) =>
  normalizeReasoningText(
    payload?.reasoning ??
      payload?.reasoning_content ??
      payload?.think_content
  );

export const extractAssistantPayloadText = (content) => {
  if (!content) return '';
  const rawText = typeof content === 'string' ? content : String(content);
  const payload = safeJsonParse(rawText);
  if (!payload) return rawText;
  const answer = extractAnswerFromPayload(payload);
  return answer || rawText;
};

export const splitThinkTaggedContent = (content) => {
  const source = extractAssistantPayloadText(content);
  if (!source) {
    return { content: '', reasoning: '' };
  }
  let cursor = 0;
  let visibleContent = '';
  let reasoningContent = '';
  let firstThinkSeen = false;
  let thinkStartedAtHead = false;
  while (cursor < source.length) {
    const thinkStart = source.indexOf(THINK_OPEN_TAG, cursor);
    if (thinkStart < 0) {
      visibleContent += source.slice(cursor);
      break;
    }
    if (!firstThinkSeen) {
      firstThinkSeen = true;
      thinkStartedAtHead = thinkStart === 0;
    }
    visibleContent += source.slice(cursor, thinkStart);
    const thinkEnd = source.indexOf(THINK_CLOSE_TAG, thinkStart + THINK_OPEN_TAG.length);
    const thinkPayloadStart = thinkStart + THINK_OPEN_TAG.length;
    if (thinkEnd < 0) {
      reasoningContent += source.slice(thinkPayloadStart);
      break;
    }
    const thinkCloseEnd = thinkEnd + THINK_CLOSE_TAG.length;
    reasoningContent += source.slice(thinkPayloadStart, thinkEnd);
    cursor = thinkCloseEnd;
  }
  if (reasoningContent && thinkStartedAtHead) {
    visibleContent = visibleContent.replace(/^\s+/, '');
  }
  return {
    content: visibleContent,
    reasoning: reasoningContent
  };
};

export const normalizeAssistantOutput = (content, reasoning = '') => {
  const normalizedReasoningRaw = normalizeReasoningText(reasoning);
  const normalizedReasoning = normalizedReasoningRaw
    ? splitThinkTaggedContent(normalizedReasoningRaw).reasoning || normalizedReasoningRaw
    : '';
  const parsed = splitThinkTaggedContent(content);
  return {
    content: parsed.content,
    inlineReasoning: parsed.reasoning,
    reasoning: normalizedReasoning || parsed.reasoning
  };
};

export const normalizeAssistantContent = (content) => normalizeAssistantOutput(content).content;

export const createThinkTagStreamParser = () => {
  let inThinkTag = false;
  let bufferedTail = '';
  const resolveSuffixLength = (text, marker) => {
    const maxLength = Math.min(Math.max(marker.length - 1, 0), text.length);
    for (let length = maxLength; length > 0; length -= 1) {
      if (marker.startsWith(text.slice(-length))) {
        return length;
      }
    }
    return 0;
  };
  const takeStableText = (source, cursor, marker) => {
    const remaining = source.slice(cursor);
    const suffixLength = resolveSuffixLength(remaining, marker);
    const safeEnd = source.length - suffixLength;
    return {
      stable: source.slice(cursor, safeEnd),
      nextCursor: source.length,
      tail: source.slice(safeEnd)
    };
  };
  const push = (chunk, flush = false) => {
    const source = `${bufferedTail}${chunk || ''}`;
    bufferedTail = '';
    if (!source) {
      return { content: '', reasoning: '' };
    }
    let contentDelta = '';
    let reasoningDelta = '';
    let cursor = 0;
    while (cursor < source.length) {
      const marker = inThinkTag ? THINK_CLOSE_TAG : THINK_OPEN_TAG;
      const markerIndex = source.indexOf(marker, cursor);
      if (markerIndex < 0) {
        if (flush) {
          const tail = source.slice(cursor);
          if (tail) {
            if (inThinkTag) {
              reasoningDelta += tail;
            } else {
              contentDelta += tail;
            }
          }
          cursor = source.length;
        } else {
          const { stable, nextCursor, tail } = takeStableText(source, cursor, marker);
          if (stable) {
            if (inThinkTag) {
              reasoningDelta += stable;
            } else {
              contentDelta += stable;
            }
          }
          bufferedTail = tail;
          cursor = nextCursor;
        }
        continue;
      }
      const stable = source.slice(cursor, markerIndex);
      if (stable) {
        if (inThinkTag) {
          reasoningDelta += stable;
        } else {
          contentDelta += stable;
        }
      }
      const markerEnd = markerIndex + marker.length;
      cursor = markerEnd;
      inThinkTag = !inThinkTag;
    }
    return {
      content: contentDelta,
      reasoning: reasoningDelta
    };
  };
  const reset = () => {
    inThinkTag = false;
    bufferedTail = '';
  };
  return { push, reset };
};

export const normalizeToolNameForFinal = (name) => {
  const raw = String(name || '').trim();
  if (!raw) return '';
  if (raw === '\u6700\u7ec8\u56de\u590d') return raw;
  return raw.toLowerCase().replace(/[\s-]+/g, '_');
};

export const isFinalToolName = (name) => {
  const normalized = normalizeToolNameForFinal(name);
  return (
    normalized === '\u6700\u7ec8\u56de\u590d' ||
    normalized === 'final_response' ||
    normalized === 'final' ||
    normalized === 'final_answer'
  );
};

export const normalizeToolCallsPayload = (toolCalls) => {
  if (!toolCalls) return [];
  let payload = toolCalls;
  if (typeof payload === 'string') {
    const parsed = safeJsonParse(payload);
    if (parsed !== null) {
      payload = parsed;
    }
  }
  if (Array.isArray(payload)) return payload;
  if (payload && typeof payload === 'object') {
    if (Array.isArray(payload.tool_calls)) return payload.tool_calls;
    if (payload.tool_calls) return [payload.tool_calls];
    if (payload.tool_call) return [payload.tool_call];
    if (payload.function_call) return [payload.function_call];
    return [payload];
  }
  return [];
};

export const parseToolCallArgs = (value) => {
  if (value === null || value === undefined) return null;
  if (typeof value === 'string') {
    const parsed = safeJsonParse(value);
    return parsed !== null ? parsed : value;
  }
  if (typeof value === 'object') return value;
  return String(value);
};

export const extractFinalAnswerFromToolCalls = (toolCalls) => {
  const calls = normalizeToolCallsPayload(toolCalls);
  for (const call of calls) {
    if (!call || typeof call !== 'object') continue;
    const functionPayload = call.function || call;
    const name = functionPayload.name || call.name || call.tool;
    if (!isFinalToolName(name)) continue;
    const argsRaw =
      functionPayload.arguments ??
      call.arguments ??
      call.args ??
      functionPayload.args ??
      functionPayload.parameters ??
      call.parameters;
    const args = parseToolCallArgs(argsRaw);
    if (typeof args === 'string') {
      const text = args.trim();
      if (text) return text;
      continue;
    }
    if (args && typeof args === 'object') {
      const answer = args.content ?? args.answer ?? args.message;
      if (answer !== undefined && answer !== null) {
        const text = String(answer).trim();
        if (text) return text;
      }
    }
  }
  return '';
};

export const resolveWorkflowRoundTimestamp = (events) => {
  if (!Array.isArray(events)) return undefined;
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const timestamp = resolveTimestampIso(events[index]?.timestamp);
    if (timestamp) return timestamp;
  }
  return undefined;
};

// Preserve full debug payloads so exported diagnostics match the real runtime data.
export const normalizeCompactionDebugText = (value) => {
  const text = String(value ?? '').trim();
  if (!text) return '';
  return text;
};

export const cloneCompactionDebugPayload = (value, fallback = null) => {
  if (value === undefined) return fallback;
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(value);
    } catch {
      // Fall back to JSON cloning for debug export compatibility.
    }
  }
  try {
    return JSON.parse(JSON.stringify(value));
  } catch {
    return value ?? fallback;
  }
};

export const buildCompactionDebugSummary = (payload) => {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
    const text = normalizeCompactionDebugText(payload);
    return text ? { value: text } : {};
  }
  const source = payload as Record<string, unknown>;
  const summary: Record<string, unknown> = {};
  const assign = (targetKey: string, ...keys: string[]) => {
    for (const key of keys) {
      const value = source[key];
      if (value === undefined || value === null || value === '') continue;
      summary[targetKey] = typeof value === 'string' ? normalizeCompactionDebugText(value) : value;
      return;
    }
  };
  assign('status', 'status');
  assign('stage', 'stage');
  assign('trigger', 'trigger');
  assign('triggerMode', 'trigger_mode', 'triggerMode');
  assign('reason', 'reason');
  assign('workflowRef', 'toolCallId', 'tool_call_id');
  assign('before', 'projected_request_tokens', 'total_tokens', 'context_tokens', 'context_guard_tokens_before');
  assign(
    'after',
    'projected_request_tokens_after',
    'total_tokens_after',
    'context_tokens_after',
    'context_guard_tokens_after',
    'final_context_tokens'
  );
  assign('messageCount', 'message_count', 'messageCount');
  assign('maxContext', 'max_context', 'maxContext', 'context_max_tokens', 'contextMaxTokens');
  assign('summaryTokens', 'summary_tokens', 'summaryTokens');
  assign('errorCode', 'error_code', 'errorCode');
  assign('errorMessage', 'error_message', 'errorMessage', 'message');
  assign('summaryText', 'summary_text', 'summaryText');
  assign('summaryModelOutput', 'summary_model_output', 'summaryModelOutput');
  return summary;
};

export const summarizeCompactionRoundEvents = (events) => {
  if (!Array.isArray(events) || events.length === 0) return null;
  const eventTypes: string[] = [];
  const progressStages: string[] = [];
  let latestCompaction: Record<string, unknown> | null = null;
  let latestContextUsage: Record<string, unknown> | null = null;
  let hasCompactionSignal = false;
  events.forEach((entry) => {
    const eventType = String(entry?.event || '').trim();
    if (!eventType) return;
    if (!eventTypes.includes(eventType)) {
      eventTypes.push(eventType);
    }
    const data =
      entry?.data && typeof entry.data === 'object' && !Array.isArray(entry.data)
        ? (entry.data as Record<string, unknown>)
        : {};
    if (eventType === 'compaction') {
      hasCompactionSignal = true;
      latestCompaction = data;
      return;
    }
    if (eventType === 'context_usage') {
      latestContextUsage = data;
      return;
    }
    if (eventType !== 'progress') {
      return;
    }
    const stage = String(data.stage || '').trim().toLowerCase();
    if (!stage) return;
    if (!progressStages.includes(stage)) {
      progressStages.push(stage);
    }
    if (stage === 'compacting' || stage === 'context_guard' || stage === 'context_overflow_recovery') {
      hasCompactionSignal = true;
    }
  });
  if (!hasCompactionSignal) return null;
  return {
    eventTypes,
    progressStages,
    latestCompaction: cloneCompactionDebugPayload(latestCompaction, {}),
    latestContextUsage: cloneCompactionDebugPayload(latestContextUsage, {})
  };
};

export const isManualCompactionRoundSummary = (summary): boolean => {
  if (!summary || typeof summary !== 'object') return false;
  const latestCompaction =
    summary.latestCompaction && typeof summary.latestCompaction === 'object'
      ? (summary.latestCompaction as Record<string, unknown>)
      : null;
  const latestContextUsage =
    summary.latestContextUsage && typeof summary.latestContextUsage === 'object'
      ? (summary.latestContextUsage as Record<string, unknown>)
      : null;
  const triggerMode = String(
    latestCompaction?.triggerMode ??
      latestCompaction?.trigger_mode ??
      latestContextUsage?.triggerMode ??
      latestContextUsage?.trigger_mode ??
      ''
  )
    .trim()
    .toLowerCase();
  return triggerMode === 'manual';
};

export const isManualCompactionRoundEvents = (events): boolean =>
  isManualCompactionRoundSummary(summarizeCompactionRoundEvents(events));

export const buildManualCompactionMarkerMessage = (roundNumber, events) => ({
  ...buildMessage('assistant', '', resolveWorkflowRoundTimestamp(events)),
  workflowItems: [],
  workflowStreaming: false,
  stream_incomplete: false,
  stream_round: roundNumber,
  manual_compaction_marker: true
});

export const insertMessageByTimestamp = (messages, message) => {
  const markerTime = resolveTimestampMs(message?.created_at);
  if (markerTime === null) {
    messages.push(message);
    return messages.length - 1;
  }
  for (let index = 0; index < messages.length; index += 1) {
    const currentTime = resolveTimestampMs(messages[index]?.created_at);
    if (currentTime === null) continue;
    if (currentTime > markerTime) {
      messages.splice(index, 0, message);
      return index;
    }
  }
  messages.push(message);
  return messages.length - 1;
};

export const summarizeCompactionWorkflowItemsForDebug = (items) => {
  if (!Array.isArray(items) || items.length === 0) {
    return [];
  }
  return items
    .map((item) => {
      if (!item || typeof item !== 'object' || Array.isArray(item)) {
        return null;
      }
      const record = item as Record<string, unknown>;
      const detail = safeJsonParse(record.detail);
      const detailSummary =
        detail && typeof detail === 'object' && !Array.isArray(detail)
          ? buildCompactionDebugSummary(detail)
          : buildCompactionDebugSummary(record.detail);
      return {
        eventType: String(record.eventType || record.event || '').trim(),
        status: String(record.status || '').trim(),
        toolCallId: String(record.toolCallId || record.tool_call_id || '').trim(),
        detail: detailSummary
      };
    })
    .filter(Boolean);
};

export const attachWorkflowEvents = (messages, rounds) => {
  if (!Array.isArray(messages) || !Array.isArray(rounds) || rounds.length === 0) {
    return messages;
  }
  const sourceMessages = messages.map((message) =>
    message && typeof message === 'object' ? { ...message } : message
  );
  const roundMap = new Map();
  rounds.forEach((round) => {
    const roundIndex = Number(round?.user_round ?? round?.round);
    if (!Number.isFinite(roundIndex)) return;
    const events = Array.isArray(round?.events) ? round.events : [];
    if (events.length) {
      roundMap.set(roundIndex, events);
    }
  });
  if (!roundMap.size) {
    return sourceMessages;
  }
  const orderedRounds = Array.from(roundMap.keys()).sort((left, right) => left - right);
  const compactionRounds = orderedRounds
    .map((roundNumber) => {
      const summary = summarizeCompactionRoundEvents(roundMap.get(roundNumber));
      if (!summary) return null;
      return { round: roundNumber, ...summary };
    })
    .filter(Boolean);
  if (compactionRounds.length > 0) {
    chatDebugLog('chat.compaction.hydrate', 'attach-workflow-events', {
      messageCount: sourceMessages.length,
      roundCount: roundMap.size,
      compactionRounds
    });
  }
  let currentRound = 0;
  let lastAssistantIndex = null;
  const assignedRounds = new Set();
  const hydratedMessages = [];
  const pushMessage = (message) => {
    hydratedMessages.push(message);
    return hydratedMessages.length - 1;
  };
  const ensureSyntheticAssistantForRound = () => {
    if (!Number.isFinite(currentRound) || currentRound <= 0 || lastAssistantIndex !== null) {
      return;
    }
    const events = roundMap.get(currentRound);
    if (!events || events.length === 0) {
      return;
    }
    if (isManualCompactionRoundEvents(events)) {
      return;
    }
    const syntheticMessage = {
      ...buildMessage('assistant', '', resolveWorkflowRoundTimestamp(events)),
      workflowItems: [],
      workflowStreaming: false,
      stream_incomplete: false,
      stream_round: currentRound
    };
    lastAssistantIndex = pushMessage(syntheticMessage);
  };
  const assignRound = (roundNumber = currentRound) => {
    if (!Number.isFinite(roundNumber) || roundNumber <= 0 || lastAssistantIndex === null) {
      return;
    }
    const events = roundMap.get(roundNumber);
    if (!events || events.length === 0) {
      return;
    }
    const manualCompactionRound = isManualCompactionRoundEvents(events);
    if (manualCompactionRound) {
      const compactionSummary = summarizeCompactionRoundEvents(events);
      if (compactionSummary) {
        chatDebugLog('chat.compaction.hydrate', 'defer-manual-round-marker', {
          round: roundNumber,
          anchorIndex: lastAssistantIndex,
          anchorCreatedAt: hydratedMessages[lastAssistantIndex]?.created_at ?? null,
          summary: compactionSummary
        });
      }
      return;
    }
    assignedRounds.add(roundNumber);
    const compactionSummary = summarizeCompactionRoundEvents(events);
    if (compactionSummary) {
      chatDebugLog('chat.compaction.hydrate', 'assign-round', {
        round: roundNumber,
        targetIndex: lastAssistantIndex,
        createdAt: hydratedMessages[lastAssistantIndex]?.created_at ?? null,
        summary: compactionSummary
      });
    }
  };
  sourceMessages.forEach((message) => {
    if (message?.role === 'user') {
      ensureSyntheticAssistantForRound();
      assignRound();
      pushMessage(message);
      currentRound += 1;
      lastAssistantIndex = null;
      return;
    }
    const index = pushMessage(message);
    if (message?.role === 'assistant') {
      lastAssistantIndex = index;
    }
  });
  ensureSyntheticAssistantForRound();
  assignRound();
  orderedRounds.forEach((roundNumber) => {
    if (assignedRounds.has(roundNumber)) {
      return;
    }
    const events = roundMap.get(roundNumber);
    if (!events || events.length === 0) {
      return;
    }
    const manualCompactionRound = isManualCompactionRoundEvents(events);
    const syntheticMessage = manualCompactionRound
      ? buildManualCompactionMarkerMessage(roundNumber, events)
      : {
          ...buildMessage('assistant', '', resolveWorkflowRoundTimestamp(events)),
          workflowItems: [],
          workflowStreaming: false,
          stream_incomplete: false,
          stream_round: roundNumber
        };
    const insertedIndex = manualCompactionRound
      ? insertMessageByTimestamp(hydratedMessages, syntheticMessage)
      : pushMessage(syntheticMessage);
    assignedRounds.add(roundNumber);
    const compactionSummary = summarizeCompactionRoundEvents(events);
    if (compactionSummary) {
      chatDebugLog(
        'chat.compaction.hydrate',
        manualCompactionRound ? 'insert-manual-round-marker' : 'append-synthetic-round',
        {
        round: roundNumber,
        targetIndex: insertedIndex,
        createdAt: syntheticMessage.created_at,
        summary: compactionSummary
        }
      );
    }
  });
  return hydratedMessages;
};

export const isFailedResult = (payload) => {
  const data = payload?.data && typeof payload.data === 'object' ? payload.data : null;
  const status = data?.status ?? payload?.status;
  if (status && String(status).toLowerCase() === 'failed') {
    return true;
  }

  const finalOk = data?.final_ok;
  if (typeof finalOk === 'boolean') {
    return !finalOk;
  }
  const ok = data?.ok;
  if (typeof ok === 'boolean') {
    if (!ok) return true;
    const directExit =
      data?.meta?.exit_code ??
      data?.exit_code ??
      data?.returncode ??
      (Array.isArray(data?.results) ? data.results[0]?.returncode : null);
    const parsedExit = Number.parseInt(String(directExit ?? ''), 10);
    if (Number.isFinite(parsedExit)) {
      return parsedExit !== 0;
    }
    return false;
  }

  return Boolean(data?.error || payload?.error);
};

export const normalizeToolCategory = (value) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (!normalized) return '';
  if (normalized.includes('builtin') || normalized.includes('built-in') || normalized.includes('built_in')) {
    return 'builtin';
  }
  if (normalized.includes('user')) {
    return 'user';
  }
  if (normalized.includes('shared')) {
    return 'shared';
  }
  if (normalized.includes('knowledge') || normalized.includes('knowledge_base') || normalized.includes('knowledgebase')) {
    return 'knowledge';
  }
  if (normalized.includes('skill')) {
    return 'skill';
  }
  if (normalized.includes('mcp')) {
    return 'mcp';
  }
  if (normalized.includes('default') || normalized === 'tool') {
    return 'default';
  }
  return '';
};

// 鏍规嵁宸ュ叿鍚嶇О涓庝簨浠跺瓧娈垫帹鏂垎绫伙紝鐢ㄤ簬宸ヤ綔娴侀珮浜?
export const resolveToolCategory = (toolName, payload) => {
  const explicit = normalizeToolCategory(
    payload?.category ??
      payload?.tool_category ??
      payload?.toolCategory ??
      payload?.tool_type ??
      payload?.toolType
  );
  if (explicit) return explicit;
  const name = String(toolName || '').trim();
  if (!name) return 'default';
  if (name.includes('@')) return 'mcp';
  const lowerName = name.toLowerCase();
  if (lowerName.includes('mcp')) return 'mcp';
  if (lowerName.includes('knowledge') || lowerName.startsWith('kb_') || name.includes('鐭ヨ瘑')) {
    return 'knowledge';
  }
  if (lowerName.includes('skill') || name.includes('\u6280\u80fd')) return 'skill';
  if (lowerName.includes('builtin') || lowerName.includes('built-in') || lowerName.includes('system')) {
    return 'builtin';
  }
  return 'default';
};
