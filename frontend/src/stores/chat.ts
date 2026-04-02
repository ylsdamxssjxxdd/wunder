import { defineStore } from 'pinia';

import {
  archiveSession as archiveSessionApi,
  cancelMessageStream,
  compactSession as compactSessionApi,
  controlSessionSubagents as controlSessionSubagentsApi,
  createSession,
  deleteSession as deleteSessionApi,
  fetchChatTransportProfile,
  getSession,
  getSessionEvents,
  getSessionHistoryPage,
  getSessionSubagents,
  listSessions,
  openChatSocket,
  renameSession as renameSessionApi,
  restoreSession as restoreSessionApi,
  resumeMessageStream,
  sendMessageStream,
  submitMessageFeedback as submitMessageFeedbackApi,
  updateSessionTools as updateSessionToolsApi
} from '@/api/chat';
import { t } from '@/i18n';
import { setDefaultSession } from '@/api/agents';
import { consumeSseStream } from '@/utils/sse';
import { formatStructuredErrorText } from '@/utils/streamError';
import { resolveCompactionProgressTitle } from '@/utils/chatCompactionUi';
import {
  didThreadRuntimeEnterBusyState,
  isSessionBusyFromSignals,
  isThreadRuntimeWaiting,
  normalizeThreadRuntimeStatus
} from '@/utils/chatSessionRuntime';
import { normalizeChatDurationSeconds, normalizeChatTimestampMs } from '@/utils/chatTiming';
import {
  normalizeMessageFeedback,
  normalizeMessageFeedbackVote,
  resolveMessageHistoryId
} from '@/utils/messageFeedback';
import { createWsMultiplexer } from '@/utils/ws';
import { isDemoMode, loadDemoChatState, saveDemoChatState } from '@/utils/demo';
import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { chatPerf } from '@/utils/chatPerf';
import { getDesktopToolCallModeForRequest, isDesktopModeEnabled } from '@/config/desktop';
import { dedupeAssistantMessages, dedupeAssistantMessagesInPlace } from './chatMessageDedup';
import {
  clearSupersededPendingAssistantMessages,
  findPendingAssistantMessage,
  stopPendingAssistantMessage
} from './chatPendingMessage';
import { consumeChatWatchChannelMessage } from './chatWatchChannelMessageRuntime';
import {
  isCompactionMarkerAssistantMessage,
  mergeCompactionMarkersIntoMessages
} from './chatCompactionMarker';
import { useCommandSessionStore } from './commandSessions';

type SnapshotAssistantMessage = {
  role: string;
  content: string;
  created_at: string;
  reasoning?: string;
  reasoningStreaming?: boolean;
  workflowStreaming?: boolean;
  stream_incomplete?: boolean;
  stream_event_id?: number;
  stream_round?: number;
  workflowItems?: unknown[];
  plan?: unknown;
  questionPanel?: unknown;
  feedback?: unknown;
  stats?: unknown;
  planVisible?: boolean;
  isGreeting?: boolean;
  attachments?: unknown[];
  subagents?: unknown[];
  hiddenInternal?: boolean;
};

type DemoChatCachePatch = {
  sessions?: unknown[];
  sessionId?: string | number | null;
  messages?: unknown[];
};

type GreetingMessageOptions = {
  greeting?: unknown;
  createdAt?: unknown;
  sessionCreatedAt?: unknown;
};

type SessionWorkflowStateOptions = {
  reset?: boolean;
};

type WorkflowEventRawPayload = {
  data: unknown;
  timestamp?: unknown;
};

type ThreadControlSession = Record<string, unknown> & {
  id: string;
  status?: unknown;
  agent_id?: unknown;
};

type WorkflowProcessorOptions = {
  finalizeWithNow?: boolean;
  streamFlushMs?: number;
  sessionId?: string | null;
  onThreadControl?: (payload: unknown) => void | Promise<void>;
  onContextUsage?: (contextTokens: number, contextTotalTokens?: number | null) => void;
};

type UsageStatsOptions = {
  updateUsage?: boolean;
  updateContextFromUsage?: boolean;
  round?: number | null;
  accumulateDurations?: boolean;
  includeInRoundAverage?: boolean;
};

type NormalizedUsagePayload = {
  input: number;
  output: number;
  total: number;
};

type QuestionPanelApplyOptions = {
  appendWorkflow?: boolean;
};

type InquiryPanelPatch = {
  status?: unknown;
  selected?: unknown[];
};

type MessageSubagentItem = {
  key: string;
  session_id: string;
  run_id: string;
  dispatch_id: string;
  title: string;
  label: string;
  status: string;
  summary: string;
  terminal: boolean;
  failed: boolean;
  canTerminate: boolean;
  updated_at: string;
  updated_at_ms: number | null;
  parent_user_round: number | null;
  parent_model_round: number | null;
  detail: Record<string, unknown>;
  agent_state: {
    status: string;
    message: string;
  };
};

type DesktopOverlayBridge = {
  showControllerHint?: (payload: {
    x: number;
    y: number;
    description?: string;
    durationMs?: number;
  }) => Promise<boolean> | boolean;
  showControllerDone?: (payload: {
    x: number;
    y: number;
    description?: string;
    durationMs?: number;
  }) => Promise<boolean> | boolean;
  showMonitorCountdown?: (payload: { waitMs: number }) => Promise<boolean> | boolean;
  hideOverlay?: () => Promise<boolean> | boolean;
};

type LoadSessionsOptions = {
  skipTransportRefresh?: boolean;
  refresh_transport?: boolean;
  agent_id?: string | number | boolean | null | undefined;
};

type ListSessionsByStatusOptions = {
  agent_id?: string | number | boolean | null | undefined;
  status?: 'active' | 'archived' | 'all' | string;
};

type OpenDraftSessionOptions = {
  agent_id?: string | number | boolean | null | undefined;
};

type SendMessageOptions = {
  attachments?: unknown[];
  suppressQueuedNotice?: boolean;
  approvalMode?: string;
  approval_mode?: string;
};

type AppendLocalMessageOptions = {
  createdAt?: unknown;
  sessionId?: unknown;
  immediate?: boolean;
};

type ResumeStreamOptions = {
  force?: boolean;
  afterEventId?: number | string;
};

type ApprovalDecision = 'approve_once' | 'approve_session' | 'deny';

type PendingApproval = {
  approval_id: string;
  request_id: string;
  session_id: string;
  tool: string;
  kind: string;
  summary: string;
  detail: unknown;
  args: unknown;
  created_at: string;
};

const buildMessageStats = () => ({
  toolCalls: 0,
  usage: null,
  prefill_duration_s: null,
  decode_duration_s: null,
  prefill_duration_total_s: null,
  decode_duration_total_s: null,
  avg_model_round_speed_tps: null,
  avg_model_round_speed_rounds: 0,
  quotaConsumed: 0,
  quotaSnapshot: null,
  contextTokens: null,
  contextTotalTokens: null,
  interaction_start_ms: null,
  interaction_end_ms: null,
  interaction_duration_s: null
});

const normalizeStatsCount = (value) => {
  if (value === null || value === undefined) return 0;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 0;
};

const parseOptionalCount = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const WORKSPACE_PATH_HINT_KEYS = [
  'path',
  'paths',
  'changed_paths',
  'changedPaths',
  'target_path',
  'targetPath',
  'source_path',
  'sourcePath',
  'destination',
  'destination_path',
  'destinationPath',
  'file',
  'files',
  'relative_path',
  'relativePath'
];

const normalizeWorkspaceEventPath = (value) => {
  const text = String(value || '').trim();
  if (!text || text === '/' || text === '.') return '';
  const normalized = text.replace(/\\/g, '/').replace(/^\/+/, '');
  return normalized === '.' ? '' : normalized;
};

// Extract path hints from heterogeneous tool/workspace payloads for incremental UI refresh.
const collectWorkspacePathHints = (...sources) => {
  const result = new Set<string>();
  const appendPathLike = (value) => {
    if (value === null || value === undefined) return;
    if (Array.isArray(value)) {
      value.forEach((item) => appendPathLike(item));
      return;
    }
    if (typeof value === 'string') {
      const normalized = normalizeWorkspaceEventPath(value);
      if (normalized || value.trim() === '/' || value.trim() === '.') {
        result.add(normalized);
      }
      return;
    }
    if (typeof value === 'object') {
      const record = value as Record<string, unknown>;
      appendPathLike(
        record.path ??
        record.relative_path ??
        record.relativePath ??
        record.target_path ??
        record.targetPath ??
        record.source_path ??
        record.sourcePath ??
        record.destination ??
        record.destination_path ??
        record.destinationPath
      );
    }
  };

  const appendFromObject = (source) => {
    if (!source || typeof source !== 'object') return;
    const record = source as Record<string, unknown>;
    WORKSPACE_PATH_HINT_KEYS.forEach((key) => appendPathLike(record[key]));
  };

  sources.forEach((source) => {
    appendFromObject(source);
    if (source && typeof source === 'object') {
      const record = source as Record<string, unknown>;
      appendFromObject(record.data);
      appendFromObject(record.meta);
    }
  });

  return Array.from(result);
};

const normalizeQuotaConsumed = (value) => {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return normalizeStatsCount(value.consumed ?? value.used ?? value.count ?? 0);
  }
  return normalizeStatsCount(value);
};

const normalizeQuotaSnapshot = (value) => {
  if (!value || typeof value !== 'object') return null;
  const daily = parseOptionalCount(
    value.daily_quota ?? value.dailyQuota ?? value.daily ?? value.quota ?? value.total
  );
  const used = parseOptionalCount(
    value.used ?? value.consumed ?? value.count ?? value.usage
  );
  const remaining = parseOptionalCount(
    value.remaining ?? value.left ?? value.quota_remaining ?? value.remain
  );
  const date = value.date ?? value.quota_date ?? value.quotaDate ?? '';
  if (daily === null && used === null && remaining === null && !date) return null;
  return {
    daily,
    used,
    remaining,
    date: date ? String(date) : ''
  };
};

const normalizeContextTokens = (value) => parseOptionalCount(value);
const normalizeContextTotalTokens = (value) => {
  const normalized = parseOptionalCount(value);
  if (normalized === null || normalized <= 0) return null;
  return normalized;
};

const normalizeDurationValue = (value) => {
  return normalizeChatDurationSeconds(value);
};

const normalizeSpeedValue = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const resolveInteractionDuration = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return null;
  }
  return (endMs - startMs) / 1000;
};

const normalizeInteractionTimestamp = (value) => {
  const millis = resolveTimestampMs(value);
  return Number.isFinite(millis) ? millis : null;
};

const normalizeSubagentEventStatus = (value) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (!normalized) return 'running';
  return normalized;
};

const normalizeMessageSubagent = (payload): MessageSubagentItem | null => {
  if (!payload || typeof payload !== 'object') return null;
  const source = payload as Record<string, unknown>;
  const agentState =
    source.agent_state && typeof source.agent_state === 'object'
      ? (source.agent_state as Record<string, unknown>)
      : source.agentState && typeof source.agentState === 'object'
        ? (source.agentState as Record<string, unknown>)
        : {};
  const sessionId = String(source.session_id ?? source.sessionId ?? '').trim();
  const runId = String(source.run_id ?? source.runId ?? '').trim();
  const key = runId || sessionId;
  if (!key) return null;
  const label = String(
    source.label ?? source.spawn_label ?? source.spawnLabel ?? source.title ?? ''
  ).trim();
  const title = label || sessionId || runId || '子智能体';
  const status = normalizeSubagentEventStatus(source.status);
  const summary = String(
    source.summary ?? agentState.message ?? source.result ?? source.error ?? ''
  ).trim();
  const updatedAtMs = normalizeInteractionTimestamp(
    source.updated_time ??
      source.updatedTime ??
      source.finished_time ??
      source.finishedTime ??
      source.started_time ??
      source.startedTime
  );
  const updatedAt = resolveTimestampIso(updatedAtMs);
  const agentStateStatus = String(agentState.status ?? status).trim();
  const agentStateMessage = String(agentState.message ?? summary).trim();
  return {
    key,
    session_id: sessionId,
    run_id: runId,
    dispatch_id: String(source.dispatch_id ?? source.dispatchId ?? '').trim(),
    title,
    label,
    status,
    summary,
    terminal: Boolean(source.terminal),
    failed: Boolean(source.failed),
    canTerminate: Boolean(source.can_terminate ?? source.canTerminate ?? !source.terminal),
    updated_at: updatedAt,
    updated_at_ms: updatedAtMs,
    parent_user_round: normalizeStreamRound(
      source.parent_user_round ?? source.parentUserRound
    ),
    parent_model_round: normalizeStreamRound(
      source.parent_model_round ?? source.parentModelRound
    ),
    detail: { ...source },
    agent_state: {
      status: agentStateStatus,
      message: agentStateMessage
    }
  };
};

const normalizeMessageSubagents = (items): MessageSubagentItem[] => {
  if (!Array.isArray(items)) return [];
  const map = new Map<string, MessageSubagentItem>();
  items.forEach((item) => {
    const normalized = normalizeMessageSubagent(item);
    if (!normalized) return;
    const existing = map.get(normalized.key);
    if (
      !existing ||
      (Number(existing.updated_at_ms || 0) <= Number(normalized.updated_at_ms || 0))
    ) {
      map.set(normalized.key, normalized);
    }
  });
  return Array.from(map.values()).sort((left, right) => {
    const leftTime = Number(left.updated_at_ms || 0);
    const rightTime = Number(right.updated_at_ms || 0);
    return rightTime - leftTime;
  });
};

const upsertMessageSubagent = (message, payload) => {
  if (!message || message.role !== 'assistant') return null;
  const normalized = normalizeMessageSubagent(payload);
  if (!normalized) return null;
  const items = normalizeMessageSubagents(message.subagents);
  const index = items.findIndex((item) => item.key === normalized.key);
  if (index >= 0) {
    items[index] = normalized;
  } else {
    items.push(normalized);
  }
  message.subagents = normalizeMessageSubagents(items);
  return normalized;
};

const attachSubagentsToMessages = (messages, subagents) => {
  if (!Array.isArray(messages) || messages.length === 0) {
    return messages;
  }
  const normalized = normalizeMessageSubagents(subagents);
  messages.forEach((message) => {
    if (!message || message.role !== 'assistant') return;
    message.subagents = [];
  });
  normalized.forEach((item) => {
    const target =
      (item.parent_user_round
        ? findAssistantMessageByUserRound(messages, item.parent_user_round)
        : null) ||
      (item.parent_model_round
        ? findAssistantMessageByRound(messages, item.parent_model_round)
        : null) ||
      [...messages].reverse().find((message) => message?.role === 'assistant');
    if (!target) return;
    upsertMessageSubagent(target, item);
  });
  return messages;
};

const normalizeUsagePayload = (payload) => {
  if (!payload || typeof payload !== 'object') return null;
  const source = payload;
  const input = Number.parseInt(
    source.input_tokens ?? source.prompt_tokens ?? source.input ?? source.prompt ?? 0,
    10
  );
  const output = Number.parseInt(
    source.output_tokens ?? source.completion_tokens ?? source.output ?? source.completion ?? 0,
    10
  );
  const totalRaw = source.total_tokens ?? source.total ?? null;
  const totalParsed = totalRaw === null || totalRaw === undefined ? null : Number.parseInt(totalRaw, 10);
  const hasInput = Number.isFinite(input) && input > 0;
  const hasOutput = Number.isFinite(output) && output > 0;
  const total =
    Number.isFinite(totalParsed) && totalParsed >= 0 ? totalParsed : (hasInput || hasOutput ? input + output : null);
  if (!hasInput && !hasOutput && total === null) {
    return null;
  }
  const normalizedInput = hasInput ? input : 0;
  let normalizedOutput = hasOutput ? output : 0;
  if (normalizedOutput <= 0 && Number.isFinite(total) && (total ?? 0) > normalizedInput) {
    normalizedOutput = Math.max(0, (total ?? 0) - normalizedInput);
  }
  return {
    input: normalizedInput,
    output: normalizedOutput,
    total: total ?? 0
  } satisfies NormalizedUsagePayload;
};

const resolveUsageContextTokens = (usage: NormalizedUsagePayload | null): number | null => {
  if (!usage) return null;
  if (Number.isFinite(usage.total) && usage.total > 0) {
    return usage.total;
  }
  if (Number.isFinite(usage.input) && usage.input > 0) {
    return usage.input;
  }
  return null;
};

const estimateStreamOutputTokens = (text) => {
  if (!text) return 0;
  const source = String(text);
  let asciiVisible = 0;
  let cjkCount = 0;
  let otherCount = 0;
  for (const char of source) {
    if (!char || /\s/.test(char)) continue;
    const code = char.charCodeAt(0);
    if (code <= 0x7f) {
      asciiVisible += 1;
      continue;
    }
    if (
      (code >= 0x4e00 && code <= 0x9fff) ||
      (code >= 0x3400 && code <= 0x4dbf) ||
      (code >= 0xf900 && code <= 0xfaff)
    ) {
      cjkCount += 1;
      continue;
    }
    otherCount += 1;
  }
  const estimated = cjkCount + asciiVisible / 4 + otherCount * 0.75;
  return Math.max(0, Math.round(estimated));
};

const normalizeMessageStats = (stats) => {
  if (!stats || typeof stats !== 'object') {
    return null;
  }
  const normalizedUsage = normalizeUsagePayload(stats.usage ?? stats.tokenUsage ?? stats.token_usage);
  const usageContextTokens = resolveUsageContextTokens(normalizedUsage);
  const hasUsageTotalTokens = Boolean(normalizedUsage && normalizedUsage.total > 0);
  const explicitContextTokens = normalizeContextTokens(
    stats.contextTokens ??
      stats.context_tokens ??
      stats.context_tokens_total ??
      stats.contextUsage ??
      stats.context_usage?.context_tokens ??
      stats.context_usage?.contextTokens
  );
  const quotaSnapshot = normalizeQuotaSnapshot(
    stats.quotaSnapshot ?? stats.quota ?? stats.quota_usage ?? stats.quotaUsage
  );
  const contextTokens = hasUsageTotalTokens
    ? usageContextTokens
    : explicitContextTokens ?? usageContextTokens;
  const contextTotalTokens = normalizeContextTotalTokens(
    stats.contextTotalTokens ??
      stats.context_total_tokens ??
      stats.context_max_tokens ??
      stats.context_window ??
      stats.max_context ??
      stats.maxContext ??
      stats.contextUsageTotal ??
      stats.context_usage?.max_context ??
      stats.context_usage?.context_max_tokens
  );
  const interactionStartMs = normalizeInteractionTimestamp(
    stats.interaction_start_ms ?? stats.interactionStartMs ?? stats.interaction_start ?? stats.started_at
  );
  const interactionEndMs = normalizeInteractionTimestamp(
    stats.interaction_end_ms ?? stats.interactionEndMs ?? stats.interaction_end ?? stats.ended_at
  );
  const interactionDuration = normalizeDurationValue(
    stats.interaction_duration_s ??
      stats.interactionDurationS ??
      stats.interactionDuration ??
      stats.duration_s ??
      stats.elapsed_s
  );
  const rangedDuration = resolveInteractionDuration(interactionStartMs, interactionEndMs);
  return {
    toolCalls: normalizeStatsCount(stats.toolCalls),
    usage: normalizedUsage,
    prefill_duration_s: normalizeDurationValue(
      stats.prefill_duration_s ?? stats.prefillDurationS ?? stats.prefillDuration
    ),
    decode_duration_s: normalizeDurationValue(
      stats.decode_duration_s ?? stats.decodeDurationS ?? stats.decodeDuration
    ),
    prefill_duration_total_s: normalizeDurationValue(
      stats.prefill_duration_total_s ?? stats.prefillDurationTotalS
    ),
    decode_duration_total_s: normalizeDurationValue(
      stats.decode_duration_total_s ?? stats.decodeDurationTotalS
    ),
    avg_model_round_speed_tps: normalizeSpeedValue(
      stats.avg_model_round_speed_tps ??
        stats.avgModelRoundSpeedTps ??
        stats.average_speed_tps ??
        stats.averageSpeedTps
    ),
    avg_model_round_speed_rounds: normalizeStatsCount(
      stats.avg_model_round_speed_rounds ??
        stats.avgModelRoundSpeedRounds ??
        stats.average_speed_rounds ??
        stats.averageSpeedRounds
    ),
    quotaConsumed: normalizeQuotaConsumed(
      stats.quotaConsumed ?? stats.quota_consumed ?? stats.quota
    ),
    quotaSnapshot,
    contextTokens,
    contextTotalTokens,
    interaction_start_ms: interactionStartMs,
    interaction_end_ms: interactionEndMs,
    interaction_duration_s: rangedDuration ?? interactionDuration
  };
};

const parseErrorText = (text) => {
  if (!text) {
    return '';
  }
  return formatStructuredErrorText(text, text);
};

const readResponseError = async (response) => {
  if (!response) {
    return '';
  }
  try {
    const text = await response.text();
    return parseErrorText(text);
  } catch (error) {
    return '';
  }
};

const ensureMessageStats = (message) => {
  if (!message || message.role !== 'assistant') return null;
  const normalized = normalizeMessageStats(message.stats);
  if (normalized) {
    message.stats = normalized;
    return normalized;
  }
  const fresh = buildMessageStats();
  message.stats = fresh;
  return fresh;
};

const mergeMessageStats = (base, incoming) => {
  const left = normalizeMessageStats(base);
  const right = normalizeMessageStats(incoming);
  if (!left && !right) return null;
  if (!left) return right;
  if (!right) return left;
  const startMs =
    left.interaction_start_ms === null || left.interaction_start_ms === undefined
      ? right.interaction_start_ms
      : right.interaction_start_ms === null || right.interaction_start_ms === undefined
        ? left.interaction_start_ms
        : Math.min(left.interaction_start_ms, right.interaction_start_ms);
  const endMs =
    left.interaction_end_ms === null || left.interaction_end_ms === undefined
      ? right.interaction_end_ms
      : right.interaction_end_ms === null || right.interaction_end_ms === undefined
        ? left.interaction_end_ms
        : Math.max(left.interaction_end_ms, right.interaction_end_ms);
  const rangedDuration = resolveInteractionDuration(startMs, endMs);
  const duration =
    rangedDuration ??
    normalizeDurationValue(
      right.interaction_duration_s ?? left.interaction_duration_s
    );
  const quotaSnapshot = right.quotaSnapshot || left.quotaSnapshot;
  const contextTokens =
    right.contextTokens === null || right.contextTokens === undefined
      ? left.contextTokens
      : right.contextTokens;
  const contextTotalTokens =
    right.contextTotalTokens === null || right.contextTotalTokens === undefined
      ? left.contextTotalTokens
      : right.contextTotalTokens;
  const leftAverageSpeedRounds = normalizeStatsCount(left.avg_model_round_speed_rounds);
  const rightAverageSpeedRounds = normalizeStatsCount(right.avg_model_round_speed_rounds);
  const preferRightAverage = rightAverageSpeedRounds >= leftAverageSpeedRounds;
  return {
    toolCalls: Math.max(left.toolCalls, right.toolCalls),
    usage: right.usage || left.usage,
    prefill_duration_s:
      right.prefill_duration_s === null || right.prefill_duration_s === undefined
        ? left.prefill_duration_s
        : right.prefill_duration_s,
    decode_duration_s:
      right.decode_duration_s === null || right.decode_duration_s === undefined
        ? left.decode_duration_s
        : right.decode_duration_s,
    prefill_duration_total_s:
      right.prefill_duration_total_s === null || right.prefill_duration_total_s === undefined
        ? left.prefill_duration_total_s
        : right.prefill_duration_total_s,
    decode_duration_total_s:
      right.decode_duration_total_s === null || right.decode_duration_total_s === undefined
        ? left.decode_duration_total_s
        : right.decode_duration_total_s,
    avg_model_round_speed_tps:
      preferRightAverage && right.avg_model_round_speed_tps !== null
        ? right.avg_model_round_speed_tps
        : left.avg_model_round_speed_tps,
    avg_model_round_speed_rounds: Math.max(
      leftAverageSpeedRounds,
      rightAverageSpeedRounds
    ),
    quotaConsumed: Math.max(left.quotaConsumed, right.quotaConsumed),
    quotaSnapshot,
    contextTokens,
    contextTotalTokens,
    interaction_start_ms: startMs,
    interaction_end_ms: endMs,
    interaction_duration_s: duration
  };
};

const resolveTimestampMs = (value) => {
  return normalizeChatTimestampMs(value);
};

const resolveTimestampIso = (value) => {
  const millis = resolveTimestampMs(value);
  return millis === null ? '' : new Date(millis).toISOString();
};

const buildMessage = (role, content, createdAt = undefined, extra = undefined) => ({
  role,
  content,
  created_at: resolveTimestampIso(createdAt) || new Date().toISOString(),
  reasoning: '',
  reasoningStreaming: false,
  plan: null,
  planVisible: false,
  questionPanel: null,
  feedback: null,
  stats: role === 'assistant' ? buildMessageStats() : null,
  ...(extra && typeof extra === 'object' ? extra : {})
});

const normalizeHiddenInternalMessage = (value) => Boolean(value);

const resolveGreetingContent = (override) => {
  const trimmed = String(override || '').trim();
  return trimmed ? trimmed : t('chat.greeting');
};
const CHAT_STATE_KEY = 'beeroom-chat-state';
const LEGACY_CHAT_STATE_KEY = 'wille-chat-state';
const DEFAULT_AGENT_KEY = '__default__';

const normalizeAgentKey = (agentId) => {
  const cleaned = String(agentId || '').trim();
  return cleaned || DEFAULT_AGENT_KEY;
};

const normalizeSessionMap = (value) => {
  if (!value || typeof value !== 'object') {
    return {};
  }
  const output = {};
  Object.entries(value).forEach(([key, sessionId]) => {
    const cleanedKey = String(key || '').trim();
    const cleanedSessionId = String(sessionId || '').trim();
    if (cleanedKey && cleanedSessionId) {
      output[cleanedKey] = cleanedSessionId;
    }
  });
  return output;
};

const buildChatPersistState = () => ({
  activeSessionId: '',
  draft: false,
  lastSessionByAgent: {}
});

const normalizeChatPersistState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildChatPersistState();
  }
  return {
    activeSessionId: typeof value.activeSessionId === 'string' ? value.activeSessionId : '',
    draft: value.draft === true,
    lastSessionByAgent: normalizeSessionMap(value.lastSessionByAgent)
  };
};

const readChatPersistState = () => {
  try {
    const raw = localStorage.getItem(CHAT_STATE_KEY) ?? localStorage.getItem(LEGACY_CHAT_STATE_KEY);
    if (!raw) return buildChatPersistState();
    if (!localStorage.getItem(CHAT_STATE_KEY)) {
      localStorage.setItem(CHAT_STATE_KEY, raw);
    }
    return normalizeChatPersistState(JSON.parse(raw));
  } catch (error) {
    return buildChatPersistState();
  }
};

const updateChatPersistState = (updater) => {
  try {
    const current = readChatPersistState();
    const next = normalizeChatPersistState(updater(current));
    localStorage.setItem(CHAT_STATE_KEY, JSON.stringify(next));
    localStorage.setItem(LEGACY_CHAT_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // ignore persistence errors
  }
};

const updateAgentSessionMap = (map, agentId, sessionId) => {
  const key = normalizeAgentKey(agentId);
  const cleanedSessionId = String(sessionId || '').trim();
  const nextMap = { ...map };
  if (cleanedSessionId) {
    nextMap[key] = cleanedSessionId;
  } else {
    delete nextMap[key];
  }
  return nextMap;
};

const persistAgentSession = (agentId, sessionId) => {
  updateChatPersistState((current) => ({
    ...current,
    lastSessionByAgent: updateAgentSessionMap(current.lastSessionByAgent, agentId, sessionId)
  }));
};

const persistActiveSession = (sessionId, agentId) => {
  const cleanedSessionId = String(sessionId || '').trim();
  updateChatPersistState((current) => ({
    ...current,
    activeSessionId: cleanedSessionId,
    draft: false,
    lastSessionByAgent: updateAgentSessionMap(current.lastSessionByAgent, agentId, cleanedSessionId)
  }));
};

const applyMainSession = (sessions, agentId, sessionId) => {
  const normalizedAgent = String(agentId || '').trim();
  const normalizedSessionId = String(sessionId || '').trim();
  return sessions.map((session) => {
    const sessionAgentId = String(session.agent_id || '').trim();
    const isMatch = normalizedAgent ? sessionAgentId === normalizedAgent : !sessionAgentId;
    if (!isMatch) return session;
    const isMain = Boolean(normalizedSessionId && session.id === normalizedSessionId);
    if (session.is_main === isMain) return session;
    return { ...session, is_main: isMain };
  });
};

const persistDraftSession = () => {
  updateChatPersistState((current) => ({
    ...current,
    activeSessionId: '',
    draft: true
  }));
};

const resolvePersistedSessionId = (agentId) => {
  const key = normalizeAgentKey(agentId);
  const state = readChatPersistState();
  return state.lastSessionByAgent?.[key] || '';
};

const CHAT_SNAPSHOT_KEY = 'beeroom-chat-snapshot';
const LEGACY_CHAT_SNAPSHOT_KEY = 'wille-chat-snapshot';
const SNAPSHOT_FLUSH_MS = 800;
const SNAPSHOT_IDLE_TIMEOUT_MS = 2000;
const SNAPSHOT_MESSAGE_LIMIT = 200;
const MAX_SNAPSHOT_MESSAGES = 50;
const SNAPSHOT_MATCH_WINDOW_MS = 2000;
let snapshotTimer = null;
let pageUnloading = false;

const getDesktopOverlayBridge = (): DesktopOverlayBridge | null => {
  if (typeof window === 'undefined') {
    return null;
  }
  const candidate = (window as Window & { wunderDesktop?: DesktopOverlayBridge }).wunderDesktop;
  return candidate || null;
};

const applyDesktopOverlayEvent = (eventType: string, data: unknown): boolean => {
  if (!isDesktopModeEnabled()) {
    return false;
  }
  const bridge = getDesktopOverlayBridge();
  if (!bridge) {
    return false;
  }
  const payload = (data && typeof data === 'object' ? data : {}) as Record<string, unknown>;
  const toNumber = (value: unknown): number | null => {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  };
  if (eventType === 'desktop_controller_hint' && typeof bridge.showControllerHint === 'function') {
    const x = toNumber(payload.x);
    const y = toNumber(payload.y);
    if (x === null || y === null) return true;
    const durationMs = toNumber(payload.duration_ms ?? payload.durationMs) ?? undefined;
    bridge.showControllerHint({
      x,
      y,
      description: typeof payload.description === 'string' ? payload.description : undefined,
      durationMs
    });
    return true;
  }
  if (eventType === 'desktop_controller_hint_done' && typeof bridge.showControllerDone === 'function') {
    const x = toNumber(payload.x);
    const y = toNumber(payload.y);
    if (x === null || y === null) return true;
    const durationMs = toNumber(payload.duration_ms ?? payload.durationMs) ?? undefined;
    bridge.showControllerDone({
      x,
      y,
      description: typeof payload.description === 'string' ? payload.description : undefined,
      durationMs
    });
    return true;
  }
  if (eventType === 'desktop_monitor_countdown' && typeof bridge.showMonitorCountdown === 'function') {
    const waitMs = toNumber(payload.wait_ms ?? payload.waitMs ?? 0) ?? 0;
    bridge.showMonitorCountdown({ waitMs });
    return true;
  }
  if (
    eventType === 'desktop_monitor_countdown_done' &&
    typeof bridge.hideOverlay === 'function'
  ) {
    bridge.hideOverlay();
    return true;
  }
  return false;
};

const normalizeStreamEventId = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const getRuntimeLastEventId = (runtime) => {
  const normalized = normalizeStreamEventId(runtime?.lastEventId);
  return normalized === null ? 0 : normalized;
};

const updateRuntimeLastEventId = (runtime, eventId) => {
  if (!runtime) return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  const current = normalizeStreamEventId(runtime.lastEventId);
  if (current === null || normalized > current) {
    runtime.lastEventId = normalized;
  }
};

const setRuntimeLastEventId = (runtime, eventId) => {
  if (!runtime) return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  runtime.lastEventId = normalized;
};

const normalizeStreamRound = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const readDeltaSegments = (value) => {
  if (!value || typeof value !== 'object') return [];
  if (Array.isArray(value.segments)) {
    return value.segments;
  }
  if (value.data && typeof value.data === 'object' && Array.isArray(value.data.segments)) {
    return value.data.segments;
  }
  return [];
};

const parseSegmentedDelta = (payload, data) => {
  const candidates = [data, payload];
  for (const source of candidates) {
    const segments = readDeltaSegments(source);
    if (!segments.length) continue;
    let delta = '';
    let reasoningDelta = '';
    let round = null;
    segments.forEach((segment) => {
      if (!segment || typeof segment !== 'object') return;
      if (typeof segment.delta === 'string' && segment.delta) {
        delta += segment.delta;
      }
      if (typeof segment.reasoning_delta === 'string' && segment.reasoning_delta) {
        reasoningDelta += segment.reasoning_delta;
      }
      if (typeof segment.think_delta === 'string' && segment.think_delta) {
        reasoningDelta += segment.think_delta;
      }
      const segmentRound = normalizeStreamRound(
        segment.user_round ?? segment.model_round ?? segment.round
      );
      if (segmentRound !== null) {
        round = segmentRound;
      }
    });
    return { delta, reasoningDelta, round };
  }
  return null;
};

const resolveEventRoundNumber = (payload, data) => {
  const directRound = normalizeStreamRound(
    data?.user_round ??
      payload?.user_round ??
      data?.model_round ??
      payload?.model_round ??
      data?.round ??
      payload?.round
  );
  if (directRound !== null) {
    return directRound;
  }
  return parseSegmentedDelta(payload, data)?.round ?? null;
};

const assignStreamEventId = (message, eventId) => {
  if (!message || typeof message !== 'object') return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  const current = normalizeStreamEventId(message.stream_event_id);
  if (current === null || normalized > current) {
    message.stream_event_id = normalized;
  }
};

const normalizeFlag = (value) => value === true || value === 'true';
const normalizeApprovalMode = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) return '';
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return '';
};

const normalizeSnapshotAttachments = (attachments) => {
  if (!Array.isArray(attachments)) return [];
  return attachments
    .map((item) => {
      if (!item || typeof item !== 'object') return null;
      const record = item;
      const name = String(record?.name || '').trim();
      const contentType = String(
        record?.content_type ?? record?.mime_type ?? record?.mimeType ?? ''
      )
        .trim()
        .toLowerCase();
      const publicPath = String(record?.public_path ?? record?.publicPath ?? '').trim();
      const rawContent = String(record?.content || '').trim();
      const content = publicPath || (!rawContent.startsWith('data:') ? rawContent : '');
      if (!name && !contentType && !publicPath && !content) return null;
      const normalized: Record<string, unknown> = {};
      if (name) normalized.name = name;
      if (contentType) normalized.content_type = contentType;
      if (publicPath) normalized.public_path = publicPath;
      if (content) normalized.content = content;
      return normalized;
    })
    .filter(Boolean);
};

const normalizeSnapshotMessage = (message) => {
  if (!message || typeof message !== 'object') return null;
  const normalizedAssistant =
    message.role === 'assistant'
      ? normalizeAssistantOutput(message.content, resolveAssistantReasoning(message))
      : null;
  const base: SnapshotAssistantMessage = {
    role: message.role,
    content:
      message.role === 'assistant'
        ? normalizedAssistant?.content || ''
        : typeof message.content === 'string'
          ? message.content
          : String(message.content || ''),
    created_at: message.created_at || ''
  };
  if (normalizeHiddenInternalMessage(message.hiddenInternal)) {
    base.hiddenInternal = true;
  }
  if (message.role === 'assistant') {
    base.reasoning = normalizedAssistant?.reasoning || '';
    base.reasoningStreaming = normalizeFlag(message.reasoningStreaming);
    base.workflowStreaming = normalizeFlag(message.workflowStreaming);
    base.stream_incomplete = normalizeFlag(message.stream_incomplete);
    const streamEventId = normalizeStreamEventId(message.stream_event_id);
    if (streamEventId !== null) {
      base.stream_event_id = streamEventId;
    }
    const streamRound = normalizeStreamRound(message.stream_round);
    if (streamRound !== null) {
      base.stream_round = streamRound;
    }
    if (Array.isArray(message.workflowItems) && message.workflowItems.length) {
      base.workflowItems = message.workflowItems;
    }
    const subagents = normalizeMessageSubagents(message.subagents);
    if (subagents.length > 0) {
      base.subagents = subagents;
    }
    const plan = normalizePlanPayload(message.plan);
    if (plan) {
      base.plan = plan;
    }
    const questionPanel = normalizeInquiryPanelState(message.questionPanel);
    if (questionPanel) {
      base.questionPanel = questionPanel;
    }
    const feedback = normalizeMessageFeedback(message.feedback);
    if (feedback) {
      base.feedback = feedback;
    }
    const stats = normalizeMessageStats(message.stats);
    if (stats) {
      base.stats = stats;
    }
    base.planVisible = shouldAutoShowPlan(plan, message);
  }
  if (message.isGreeting) {
    base.isGreeting = true;
  }
  const attachments = normalizeSnapshotAttachments(message.attachments);
  if (attachments.length) {
    base.attachments = attachments;
  }
  return base;
};

const buildSnapshotMessages = (messages = []) => {
  const sliced = messages.slice(-MAX_SNAPSHOT_MESSAGES);
  return sliced
    .map((message) => {
      const normalized = normalizeSnapshotMessage(message);
      if (!normalized) return null;
      const hasWorkflowItems =
        Array.isArray(normalized.workflowItems) && normalized.workflowItems.length > 0;
      const shouldKeepWorkflow =
        hasWorkflowItems || normalized.stream_incomplete || normalized.workflowStreaming;
      if (!shouldKeepWorkflow) {
        delete normalized.workflowItems;
      }
      if (Array.isArray(normalized.subagents) && normalized.subagents.length === 0) {
        delete normalized.subagents;
      }
      return normalized;
    })
    .filter(Boolean);
};

const buildChatSnapshot = (storeState) => {
  const sessionId = String(storeState.activeSessionId || '');
  if (!sessionId) return null;
  const sourceMessages = Array.isArray(storeState.messages) ? storeState.messages : [];
  const trimmed =
    sourceMessages.length > SNAPSHOT_MESSAGE_LIMIT
      ? sourceMessages.slice(-SNAPSHOT_MESSAGE_LIMIT)
      : sourceMessages;
  const messages = buildSnapshotMessages(trimmed);
  if (!messages.length) return null;
  return {
    sessionId,
    messages,
    updatedAt: Date.now()
  };
};

const readChatSnapshot = () => {
  try {
    const raw = localStorage.getItem(CHAT_SNAPSHOT_KEY) ?? localStorage.getItem(LEGACY_CHAT_SNAPSHOT_KEY);
    if (!raw) return null;
    if (!localStorage.getItem(CHAT_SNAPSHOT_KEY)) {
      localStorage.setItem(CHAT_SNAPSHOT_KEY, raw);
    }
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return null;
    const sessionId = String(parsed.sessionId || '');
    const messages = Array.isArray(parsed.messages) ? parsed.messages : [];
    if (!sessionId || !messages.length) return null;
    return {
      sessionId,
      messages
    };
  } catch (error) {
    return null;
  }
};

const writeChatSnapshot = (payload) => {
  if (!payload) return;
  try {
    const serialized = JSON.stringify(payload);
    localStorage.setItem(CHAT_SNAPSHOT_KEY, serialized);
    localStorage.setItem(LEGACY_CHAT_SNAPSHOT_KEY, serialized);
  } catch (error) {
    // ignore persistence errors
  }
};

const clearChatSnapshot = (sessionId) => {
  try {
    const current = readChatSnapshot();
    if (!current || current.sessionId !== String(sessionId || '')) return;
    localStorage.removeItem(CHAT_SNAPSHOT_KEY);
    localStorage.removeItem(LEGACY_CHAT_SNAPSHOT_KEY);
  } catch (error) {
    // ignore storage errors
  }
};

const clearAllChatSnapshots = () => {
  try {
    localStorage.removeItem(CHAT_SNAPSHOT_KEY);
    localStorage.removeItem(LEGACY_CHAT_SNAPSHOT_KEY);
  } catch (error) {
    // ignore storage errors
  }
};

const scheduleChatSnapshot = (storeState, immediate = false) => {
  const flush = () => {
    if (!chatPerf.enabled()) {
      const snapshot = buildChatSnapshot(storeState);
      if (snapshot) {
        writeChatSnapshot(snapshot);
      }
      return;
    }
    const start = performance.now();
    const snapshot = buildChatSnapshot(storeState);
    if (snapshot) {
      writeChatSnapshot(snapshot);
    }
    chatPerf.recordDuration('chat_snapshot_flush', performance.now() - start, {
      messageCount: Array.isArray(storeState?.messages) ? storeState.messages.length : 0
    });
  };
  if (immediate) {
    flush();
    return;
  }
  if (snapshotTimer !== null) return;
  snapshotTimer = setTimeout(() => {
    snapshotTimer = null;
    if (typeof requestIdleCallback === 'function') {
      requestIdleCallback(
        () => {
          flush();
        },
        { timeout: SNAPSHOT_IDLE_TIMEOUT_MS }
      );
      return;
    }
    flush();
  }, SNAPSHOT_FLUSH_MS);
};

const mergeSnapshotAssistant = (target, snapshot) => {
  if (!target || target.role !== 'assistant' || !snapshot) return false;
  const snapshotContent = String(snapshot.content || '');
  const serverContent = String(target.content || '');
  const snapshotEventId = normalizeStreamEventId(snapshot.stream_event_id);
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const snapshotRound = normalizeStreamRound(snapshot.stream_round);
  const targetRound = normalizeStreamRound(target.stream_round);
  const snapshotPlan = normalizePlanPayload(snapshot.plan);
  const hasSnapshotPlan = hasPlanSteps(snapshotPlan);
  const snapshotFeedback = normalizeMessageFeedback(snapshot.feedback);
  const snapshotStats = normalizeMessageStats(snapshot.stats);
  const snapshotSubagents = normalizeMessageSubagents(snapshot.subagents);
  const hasWorkflowItems =
    Array.isArray(snapshot.workflowItems) && snapshot.workflowItems.length > 0;
  const shouldMergeContent =
    snapshotContent.length > serverContent.length ||
    (snapshot.stream_incomplete && serverContent.length === 0);
  const shouldMergeFlags = Boolean(
    snapshot.stream_incomplete ||
      snapshot.workflowStreaming ||
      snapshot.reasoningStreaming ||
      hasWorkflowItems ||
      hasSnapshotPlan ||
      snapshotRound !== null ||
      snapshotEventId !== null ||
      snapshot.questionPanel ||
      snapshotFeedback ||
      snapshotSubagents.length > 0
  );
  if (!shouldMergeContent && !shouldMergeFlags && !snapshotStats) {
    return false;
  }
  if (shouldMergeContent && snapshotContent) {
    target.content = snapshotContent;
  }
  if (snapshot.reasoning) {
    target.reasoning = snapshot.reasoning;
  }
  if (shouldMergeFlags) {
    target.reasoningStreaming =
      normalizeFlag(snapshot.reasoningStreaming) ||
      normalizeFlag(target.reasoningStreaming);
    if (hasWorkflowItems) {
      const targetHasItems =
        Array.isArray(target.workflowItems) && target.workflowItems.length > 0;
      if (!targetHasItems || snapshot.workflowItems.length >= target.workflowItems.length) {
        target.workflowItems = snapshot.workflowItems;
      }
    }
    target.workflowStreaming =
      normalizeFlag(snapshot.workflowStreaming) ||
      normalizeFlag(target.workflowStreaming);
    target.stream_incomplete =
      normalizeFlag(snapshot.stream_incomplete) ||
      normalizeFlag(target.stream_incomplete);
    if (snapshotRound !== null && (targetRound === null || snapshotRound > targetRound)) {
      target.stream_round = snapshotRound;
    }
    if (snapshotEventId !== null && (targetEventId === null || snapshotEventId > targetEventId)) {
      target.stream_event_id = snapshotEventId;
    }
    const panel = normalizeInquiryPanelState(snapshot.questionPanel);
    if (panel) {
      target.questionPanel = panel;
    }
    if (snapshotPlan) {
      target.plan = snapshotPlan;
      target.planVisible =
        Boolean(target.planVisible) || shouldAutoShowPlan(snapshotPlan, snapshot);
    }
    if (snapshotFeedback) {
      target.feedback = snapshotFeedback;
    }
    if (snapshotSubagents.length > 0) {
      target.subagents = snapshotSubagents;
    }
  }
  if (snapshotStats) {
    target.stats = mergeMessageStats(target.stats, snapshotStats);
  }
  return true;
};

const findSnapshotAssistantIndex = (target, snapshotAssistants, cursor) => {
  if (!target || cursor < 0) return -1;
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const targetRound = normalizeStreamRound(target.stream_round);
  const targetTime = resolveTimestampMs(target.created_at);
  const targetContent = normalizeAssistantContent(target.content || '');
  let bestIndex = -1;
  let bestDelta = Infinity;
  for (let i = cursor; i >= 0; i -= 1) {
    const snapshot = snapshotAssistants[i];
    if (!snapshot) continue;
    const snapshotEventId = normalizeStreamEventId(snapshot.stream_event_id);
    if (targetEventId !== null && snapshotEventId !== null && snapshotEventId === targetEventId) {
      return i;
    }
    const snapshotRound = normalizeStreamRound(snapshot.stream_round);
    if (targetRound !== null && snapshotRound !== null && snapshotRound === targetRound) {
      return i;
    }
    const snapshotTime = resolveTimestampMs(snapshot.created_at);
    if (Number.isFinite(targetTime) && Number.isFinite(snapshotTime)) {
      const delta = Math.abs(targetTime - snapshotTime);
      if (delta <= SNAPSHOT_MATCH_WINDOW_MS && delta < bestDelta) {
        bestDelta = delta;
        bestIndex = i;
        if (delta === 0) break;
      }
      continue;
    }
    if (targetContent) {
      const snapshotContent = normalizeAssistantContent(snapshot.content || '');
      if (
        snapshotContent &&
        (targetContent.includes(snapshotContent) || snapshotContent.includes(targetContent))
      ) {
        return i;
      }
    }
  }
  if (bestIndex >= 0) return bestIndex;
  const isPendingTarget =
    normalizeFlag(target.stream_incomplete) || !normalizeAssistantContent(target.content || '');
  if (isPendingTarget || !Number.isFinite(targetTime)) {
    return cursor;
  }
  return -1;
};

const mergeSnapshotIntoMessages = (messages, snapshot) => {
  if (!snapshot || !Array.isArray(snapshot.messages) || snapshot.messages.length === 0) {
    return messages;
  }
  if (!Array.isArray(messages) || messages.length === 0) {
    return snapshot.messages.map((item) => normalizeSnapshotMessage(item)).filter(Boolean);
  }
  const snapshotMessages = snapshot.messages
    .map((item) => normalizeSnapshotMessage(item))
    .filter(Boolean);
  if (!snapshotMessages.length) {
    return messages;
  }
  const snapshotAssistants = snapshotMessages.filter(
    (message) => message.role === 'assistant' && !message.isGreeting
  );
  if (!snapshotAssistants.length) return messages;
  let cursor = snapshotAssistants.length - 1;
  for (let i = messages.length - 1; i >= 0 && cursor >= 0; i -= 1) {
    const target = messages[i];
    if (target?.role !== 'assistant') {
      continue;
    }
    const matchIndex = findSnapshotAssistantIndex(target, snapshotAssistants, cursor);
    if (matchIndex < 0) {
      continue;
    }
    mergeSnapshotAssistant(target, snapshotAssistants[matchIndex]);
    cursor = matchIndex - 1;
  }
  return messages;
};

// 演示模式聊天缓存结构（仅用于本地暂存）
const buildDemoChatState = () => ({
  sessions: [],
  messages: {}
});

const normalizeDemoChatState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildDemoChatState();
  }
  return {
    sessions: Array.isArray(value.sessions) ? value.sessions : [],
    messages: value.messages && typeof value.messages === 'object' ? value.messages : {}
  };
};

const getDemoChatState = () => normalizeDemoChatState(loadDemoChatState());

const persistDemoChatState = (state) => saveDemoChatState(state);

const syncDemoChatCache = ({ sessions, sessionId, messages }: DemoChatCachePatch = {}) => {
  if (!isDemoMode()) return;
  const state = getDemoChatState();
  if (Array.isArray(sessions)) {
    state.sessions = sessions;
  }
  if (sessionId) {
    state.messages = state.messages || {};
    state.messages[sessionId] = Array.isArray(messages) ? messages : [];
  }
  persistDemoChatState(state);
};

const removeDemoChatSession = (sessionId) => {
  if (!isDemoMode() || !sessionId) return;
  const state = getDemoChatState();
  state.sessions = (state.sessions || []).filter((item) => item.id !== sessionId);
  if (state.messages?.[sessionId]) {
    delete state.messages[sessionId];
  }
  persistDemoChatState(state);
};

const resolveSessionActivityTime = (session) =>
  resolveTimestampMs(
    session?.updated_at ?? session?.last_message_at ?? session?.created_at
  );

const sortSessionsByActivity = (sessions = []) =>
  (Array.isArray(sessions) ? sessions.slice() : [])
    .map((session, index) => ({ session, index }))
    .sort((a, b) => {
      const aTime = resolveSessionActivityTime(a.session);
      const bTime = resolveSessionActivityTime(b.session);
      if (aTime !== null && bTime !== null && aTime !== bTime) {
        return bTime - aTime;
      }
      if (aTime !== null && bTime === null) return -1;
      if (aTime === null && bTime !== null) return 1;
      return a.index - b.index;
    })
    .map((item) => item.session);

const buildGreetingMessage = (createdAt, greeting) => ({
  ...buildMessage('assistant', resolveGreetingContent(greeting), createdAt),
  workflowItems: [],
  workflowStreaming: false,
  isGreeting: true
});

const resolveGreetingTimestamp = (messages, createdAt) => {
  const direct = resolveTimestampIso(createdAt);
  if (direct) return direct;
  const safeMessages = Array.isArray(messages) ? messages : [];
  const candidate = safeMessages.find((message) => message?.created_at)?.created_at;
  return resolveTimestampIso(candidate);
};

const ensureGreetingMessage = (messages, options: GreetingMessageOptions = {}) => {
  const safeMessages = Array.isArray(messages) ? messages : [];
  const greetingText = resolveGreetingContent(options?.greeting);
  // 无论历史会话与否，都补一条问候语，保证提示词预览入口稳定可见
  const greetingIndex = safeMessages.findIndex((message) => message?.isGreeting);
  if (greetingIndex >= 0) {
    if (safeMessages[greetingIndex]?.content !== greetingText) {
      safeMessages[greetingIndex].content = greetingText;
    }
    const createdAt = options?.createdAt ?? options?.sessionCreatedAt;
    if (createdAt) {
      const greetingAt = resolveGreetingTimestamp(safeMessages, createdAt);
      if (greetingAt) {
        const currentAt = resolveTimestampIso(safeMessages[greetingIndex]?.created_at);
        if (currentAt !== greetingAt) {
          safeMessages[greetingIndex].created_at = greetingAt;
        }
      }
    }
    return safeMessages;
  }
  const greetingAt = resolveGreetingTimestamp(safeMessages, options?.createdAt ?? options?.sessionCreatedAt);
  return [buildGreetingMessage(greetingAt, greetingText), ...safeMessages];
};

const safeJsonParse = (raw) => {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch (error) {
    return null;
  }
};

const normalizeApprovalKind = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'exec' || raw === 'patch') {
    return raw;
  }
  return '';
};

const normalizePendingApproval = (payload, requestId, sessionId): PendingApproval | null => {
  if (!payload || typeof payload !== 'object') {
    return null;
  }
  const approvalId = String(payload.approval_id ?? payload.id ?? '').trim();
  if (!approvalId) {
    return null;
  }
  const summary = String(payload.summary || '').trim();
  const tool = String(payload.tool || payload.name || '').trim();
  const kind = normalizeApprovalKind(payload.kind);
  const normalizedSessionId = String(
    payload.session_id ?? payload.sessionId ?? sessionId ?? ''
  ).trim();
  if (!normalizedSessionId) {
    return null;
  }
  return {
    approval_id: approvalId,
    request_id: String(requestId || '').trim(),
    session_id: normalizedSessionId,
    tool,
    kind,
    summary: summary || tool || approvalId,
    detail: payload.detail ?? null,
    args: payload.args ?? null,
    created_at: new Date().toISOString()
  };
};

const normalizeApprovalResultId = (payload) => {
  if (!payload || typeof payload !== 'object') return '';
  return String(payload.approval_id ?? payload.id ?? '').trim();
};

const stringifyPayload = (payload) => {
  if (payload === null || payload === undefined) return '';
  if (typeof payload === 'string') return payload;
  try {
    return JSON.stringify(payload, null, 2);
  } catch (error) {
    return String(payload);
  }
};

const tailText = (text, maxLength = 240) => {
  if (!text) return '';
  return text.length > maxLength ? `...${text.slice(-maxLength)}` : text;
};

const normalizePlanStatus = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) return 'pending';
  const normalized = raw.replace(/[-\s]+/g, '_');
  if (normalized === 'pending') return 'pending';
  if (normalized === 'in_progress' || normalized === 'inprogress') return 'in_progress';
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'done') return 'completed';
  return 'pending';
};

const normalizePlanPayload = (payload) => {
  if (!payload) return null;
  const rawPlan = Array.isArray(payload?.plan)
    ? payload.plan
    : Array.isArray(payload?.steps)
      ? payload.steps
      : Array.isArray(payload)
        ? payload
        : [];
  if (!rawPlan.length) return null;
  const explanation = typeof payload?.explanation === 'string' ? payload.explanation.trim() : '';
  const steps = [];
  let hasInProgress = false;
  rawPlan.forEach((item) => {
    if (!item) return;
    const step = String(item?.step ?? item?.title ?? item).trim();
    if (!step) return;
    let status = normalizePlanStatus(item?.status);
    if (status === 'in_progress') {
      if (hasInProgress) {
        status = 'pending';
      } else {
        hasInProgress = true;
      }
    }
    steps.push({ step, status });
  });
  if (!steps.length) return null;
  return {
    explanation,
    steps
  };
};

const isRecommendedLabel = (label) => {
  const normalized = String(label || '').trim();
  if (!normalized) return false;
  const lowered = normalized.toLowerCase();
  const keywords = new Set(['推荐', 'recommended', t('chat.inquiry.recommended')]);
  for (const keyword of keywords) {
    if (!keyword) continue;
    if (lowered.includes(String(keyword).toLowerCase())) {
      return true;
    }
  }
  return false;
};

const normalizeInquiryRoutes = (routes) =>
  (Array.isArray(routes) ? routes : [])
    .map((item) => {
      if (!item) return null;
      if (typeof item === 'string') {
        const label = item.trim();
        if (!label) return null;
        return { label, description: '', recommended: isRecommendedLabel(label) };
      }
      if (typeof item !== 'object') return null;
      const label = String(item.label ?? item.title ?? item.name ?? '').trim();
      if (!label) return null;
      const description = String(item.description ?? item.detail ?? item.desc ?? item.summary ?? '').trim();
      return {
        label,
        description,
        recommended: Boolean(item.recommended ?? item.preferred) || isRecommendedLabel(label)
      };
    })
    .filter(Boolean);

const normalizeInquiryPanelPayload = (payload) => {
  if (!payload || typeof payload !== 'object') return null;
  const question = String(
    payload.question ?? payload.prompt ?? payload.title ?? payload.header ?? ''
  ).trim();
  let normalizedRoutes = normalizeInquiryRoutes(payload.routes);
  if (!normalizedRoutes.length) {
    normalizedRoutes = normalizeInquiryRoutes(payload.options);
  }
  if (!normalizedRoutes.length) {
    normalizedRoutes = normalizeInquiryRoutes(payload.choices);
  }
  normalizedRoutes = Array.isArray(normalizedRoutes) ? normalizedRoutes.filter(Boolean) : [];
  if (normalizedRoutes.length === 0) {
    return null;
  }
  const keepOpenRaw = payload.keep_open ?? payload.keepOpen ?? payload.awaiting;
  const keepOpen = keepOpenRaw === undefined ? true : keepOpenRaw === true;
  return {
    question: question || t('chat.inquiry.defaultQuestion'),
    routes: normalizedRoutes,
    multiple: payload.multiple === true || payload.allow_multiple === true || payload.multi === true,
    keepOpen
  };
};

const normalizeInquiryPanelStatus = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'answered') return 'answered';
  if (raw === 'dismissed') return 'dismissed';
  return 'pending';
};

const normalizeInquiryPanelState = (panel) => {
  const normalized = normalizeInquiryPanelPayload(panel);
  if (!normalized) return null;
  const status = normalizeInquiryPanelStatus(panel?.status);
  const selected = Array.isArray(panel?.selected)
    ? panel.selected.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  return { ...normalized, status, selected };
};

const dismissStaleInquiryPanels = (messages = []) => {
  if (!Array.isArray(messages)) return false;
  let hasUserAfter = false;
  let updated = false;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (!message) continue;
    if (message.role === 'user') {
      hasUserAfter = true;
      continue;
    }
    if (message.role !== 'assistant') {
      continue;
    }
    const panel = normalizeInquiryPanelState(message.questionPanel);
    if (!panel) continue;
    if (
      panel.status === 'pending' &&
      (hasUserAfter || (!panel.keepOpen && !isMessageRunning(message)))
    ) {
      message.questionPanel = { ...panel, status: 'dismissed' };
      updated = true;
      continue;
    }
    message.questionPanel = panel;
  }
  return updated;
};

const isQuestionPanelToolName = (name) => {
  const raw = String(name || '').trim();
  if (!raw) return false;
  if (raw === '问询面板') return true;
  const lower = raw.toLowerCase();
  return lower === 'question_panel' || lower === 'ask_panel';
};

const hasPlanSteps = (plan) => Array.isArray(plan?.steps) && plan.steps.length > 0;
const isMessageRunning = (message) =>
  normalizeFlag(message?.stream_incomplete) || normalizeFlag(message?.workflowStreaming);
const shouldAutoShowPlan = (plan, message) => hasPlanSteps(plan) && isMessageRunning(message);

const applyPlanUpdate = (assistantMessage, payload) => {
  if (!assistantMessage || assistantMessage.role !== 'assistant') return null;
  const normalized = normalizePlanPayload(payload);
  if (!normalized) return null;
  assistantMessage.plan = normalized;
  assistantMessage.planVisible =
    Boolean(assistantMessage.planVisible) || shouldAutoShowPlan(normalized, assistantMessage);
  return normalized;
};

const buildWorkflowItem = (title, detail, status = 'completed', meta = {}) => ({
  id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
  title,
  detail,
  status,
  ...meta
});

const hydrateSessionCommandSessions = (sessionId, snapshots) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId || !Array.isArray(snapshots)) return;
  useCommandSessionStore().hydrateSession(targetId, snapshots);
};

const upsertCommandSessionRuntime = (sessionId, payload) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId || !payload) return null;
  return useCommandSessionStore().upsertSnapshot(targetId, payload);
};

const appendCommandSessionRuntimeDelta = (
  sessionId,
  commandSessionId,
  stream,
  delta,
  meta = {}
) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId || !commandSessionId || !delta) return null;
  return useCommandSessionStore().appendDelta(targetId, commandSessionId, stream, delta, meta);
};

const clearSessionCommandSessions = (sessionId) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId) return;
  useCommandSessionStore().clearSession(targetId);
};

// 会话级模型轮次状态，保证同一会话的轮次连续递增
const sessionWorkflowState = new Map();

const buildSessionWorkflowState = () => ({
  globalRound: 0,
  currentRound: null
});

const normalizeSessionWorkflowState = (state) => {
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

const getSessionWorkflowState = (sessionId, options: SessionWorkflowStateOptions = {}) => {
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

const updateWorkflowItem = (items, id, patch) => {
  const target = items.find((item) => item.id === id);
  if (target) {
    Object.assign(target, patch);
  }
};

const resolveEventType = (eventName, payload) => {
  // SSE 事件名优先，但遇到默认的 message 时允许使用 payload 内部字段
  const normalized = (eventName || '').trim();
  if (normalized && normalized !== 'message') return normalized;
  if (payload?.event) return payload.event;
  if (payload?.type) return payload.type;
  return normalized || 'message';
};

const handleApprovalEvent = (store, eventType, payload, requestId, sessionId) => {
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

const pickText = (value, fallback = '') => {
  if (value === null || value === undefined) return fallback;
  if (typeof value === 'string') return value;
  return stringifyPayload(value);
};

// 保留完整详情，供弹窗查看完整内容
const pickString = (...values) => {
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

const toOptionalInt = (...values) => {
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

const buildDetail = (payload) => stringifyPayload(payload);

const defaultSessionTitles = new Set(['新会话', '未命名会话']);

const buildSessionTitle = (content, maxLength = 20) => {
  const cleaned = String(content || '').trim().replace(/\s+/g, ' ');
  if (!cleaned) return '';
  if (cleaned.length <= maxLength) return cleaned;
  return `${cleaned.slice(0, maxLength)}...`;
};

const shouldAutoTitle = (title) => {
  if (!title) return true;
  return defaultSessionTitles.has(String(title).trim());
};

const extractAnswerFromPayload = (payload) => {
  if (!payload || typeof payload !== 'object') return '';
  const data = payload.data;
  if (data && typeof data === 'object') {
    const answer = data.answer || data.content || data.message;
    if (answer) return String(answer);
  }
  const answer = payload.answer || payload.content || payload.message;
  return answer ? String(answer) : '';
};

const THINK_OPEN_TAG = '<think>';
const THINK_CLOSE_TAG = '</think>';

const normalizeReasoningText = (value) =>
  typeof value === 'string' ? value : value ? String(value) : '';

const resolveAssistantReasoning = (payload) =>
  normalizeReasoningText(
    payload?.reasoning ??
      payload?.reasoning_content ??
      payload?.think_content
  );

const extractAssistantPayloadText = (content) => {
  if (!content) return '';
  const rawText = typeof content === 'string' ? content : String(content);
  const payload = safeJsonParse(rawText);
  if (!payload) return rawText;
  const answer = extractAnswerFromPayload(payload);
  return answer || rawText;
};

const splitThinkTaggedContent = (content) => {
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

const normalizeAssistantOutput = (content, reasoning = '') => {
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

const normalizeAssistantContent = (content) => normalizeAssistantOutput(content).content;

const createThinkTagStreamParser = () => {
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

const normalizeToolNameForFinal = (name) => {
  const raw = String(name || '').trim();
  if (!raw) return '';
  if (raw === '最终回复') return raw;
  return raw.toLowerCase().replace(/[\s-]+/g, '_');
};

const isFinalToolName = (name) => {
  const normalized = normalizeToolNameForFinal(name);
  return (
    normalized === '最终回复' ||
    normalized === 'final_response' ||
    normalized === 'final' ||
    normalized === 'final_answer'
  );
};

const normalizeToolCallsPayload = (toolCalls) => {
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

const parseToolCallArgs = (value) => {
  if (value === null || value === undefined) return null;
  if (typeof value === 'string') {
    const parsed = safeJsonParse(value);
    return parsed !== null ? parsed : value;
  }
  if (typeof value === 'object') return value;
  return String(value);
};

const extractFinalAnswerFromToolCalls = (toolCalls) => {
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

const buildWorkflowEventRaw = (data, timestamp = undefined) => {
  const payload: WorkflowEventRawPayload = { data: data ?? null };
  if (timestamp) {
    payload.timestamp = timestamp;
  }
  return JSON.stringify(payload);
};

const normalizeWorkflowEvents = (events, message) => {
  if (!Array.isArray(events) || events.length === 0) {
    return [];
  }
  const normalizedOutput = normalizeAssistantOutput(
    message?.content || '',
    resolveAssistantReasoning(message)
  );
  const content = normalizedOutput.content;
  const reasoning = normalizedOutput.reasoning;
  const normalized = [];
  events.forEach((event) => {
    const eventName = String(event?.event || '').trim();
    if (!eventName || eventName === 'final') {
      return;
    }
    let data = event?.data ?? null;
    if (eventName === 'llm_output' && (content || reasoning)) {
      if (data && typeof data === 'object' && !Array.isArray(data)) {
        data = { ...data, content, reasoning };
      } else {
        data = { content, reasoning };
      }
    }
    normalized.push({
      event: eventName,
      raw: buildWorkflowEventRaw(data, event?.timestamp)
    });
  });
  if (content || reasoning) {
    normalized.push({
      event: 'final',
      raw: buildWorkflowEventRaw({ answer: content, content, reasoning })
    });
  }
  return normalized;
};

const resolveWorkflowRoundTimestamp = (events) => {
  if (!Array.isArray(events)) return undefined;
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const timestamp = resolveTimestampIso(events[index]?.timestamp);
    if (timestamp) return timestamp;
  }
  return undefined;
};

const attachWorkflowEvents = (messages, rounds) => {
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
  let currentRound = 0;
  let lastAssistantIndex = null;
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
    const syntheticMessage = {
      ...buildMessage('assistant', '', resolveWorkflowRoundTimestamp(events)),
      workflowItems: [],
      workflowStreaming: false,
      stream_incomplete: false,
      stream_round: currentRound
    };
    lastAssistantIndex = pushMessage(syntheticMessage);
  };
  const assignRound = () => {
    if (!Number.isFinite(currentRound) || currentRound <= 0 || lastAssistantIndex === null) {
      return;
    }
    const events = roundMap.get(currentRound);
    if (!events || events.length === 0) {
      return;
    }
    const target = hydratedMessages[lastAssistantIndex];
    target.workflow_events = normalizeWorkflowEvents(events, target);
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
  return hydratedMessages;
};

const isFailedResult = (payload) => {
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

const normalizeToolCategory = (value) => {
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

// 根据工具名称与事件字段推断分类，用于工作流高亮
const resolveToolCategory = (toolName, payload) => {
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
  if (lowerName.includes('knowledge') || lowerName.startsWith('kb_') || name.includes('知识')) {
    return 'knowledge';
  }
  if (lowerName.includes('skill') || name.includes('技能')) return 'skill';
  if (lowerName.includes('builtin') || lowerName.includes('built-in') || lowerName.includes('system')) {
    return 'builtin';
  }
  return 'default';
};

const sessionRuntime = new Map();
const sessionMessages = new Map();
const sessionListCache = new Map();
const sessionListCacheInFlight = new Map();
const sessionDetailPrefetchInFlight = new Map();
const sessionSubagentsInFlight = new Map();
const sessionDetailWarmState = new Map();
const sessionHistoryState = new Map();

const SESSION_LIST_CACHE_TTL_MS = 15 * 1000;
const SESSION_DETAIL_WARM_TTL_MS = 20 * 1000;

const resolveSessionKey = (sessionId) => String(sessionId || '').trim();

const buildHistoryState = () => ({
  beforeId: null,
  hasMore: true,
  loading: false,
  windowLimit: MESSAGE_WINDOW_LIMIT
});

const getHistoryState = (sessionId, options: { reset?: boolean } = {}) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return buildHistoryState();
  const reset = options.reset === true;
  let state = sessionHistoryState.get(key);
  if (!state || reset) {
    state = buildHistoryState();
    sessionHistoryState.set(key, state);
  }
  return state;
};

const updateHistoryState = (sessionId, patch) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  const state = getHistoryState(key);
  Object.assign(state, patch);
  return state;
};

const findOldestHistoryId = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = 0; i < messages.length; i += 1) {
    const message = messages[i];
    const id = Number.parseInt(String(message?.history_id ?? ''), 10);
    if (Number.isFinite(id) && id > 0) {
      return id;
    }
  }
  return null;
};

const applyMessageFeedbackByHistoryId = (messages, historyId, feedback) => {
  if (!Array.isArray(messages)) return false;
  const normalizedHistoryId = Number.parseInt(String(historyId ?? ''), 10);
  if (!Number.isFinite(normalizedHistoryId) || normalizedHistoryId <= 0) {
    return false;
  }
  const normalizedFeedback = normalizeMessageFeedback(feedback);
  if (!normalizedFeedback) return false;
  let updated = false;
  for (let i = 0; i < messages.length; i += 1) {
    const message = messages[i];
    if (!message || message.role !== 'assistant') continue;
    if (resolveMessageHistoryId(message) !== normalizedHistoryId) continue;
    const current = normalizeMessageFeedback(message.feedback);
    const shouldUpdate =
      !current ||
      current.vote !== normalizedFeedback.vote ||
      current.locked !== true ||
      String(current.created_at || '') !== String(normalizedFeedback.created_at || '');
    message.feedback = {
      ...normalizedFeedback,
      locked: true
    };
    if (shouldUpdate) {
      updated = true;
    }
  }
  return updated;
};

const normalizeFeedbackMatchText = (value) =>
  normalizeAssistantContent(String(value || ''))
    .replace(/\s+/g, ' ')
    .trim();

const isAssistantFeedbackCandidate = (message) => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return false;
  if (resolveMessageHistoryId(message) > 0) return false;
  const text = normalizeFeedbackMatchText(message.content);
  return Boolean(text || message.created_at);
};

const scoreAssistantHistoryMatch = (localMessage, remoteMessage) => {
  const localText = normalizeFeedbackMatchText(localMessage?.content);
  const remoteText = normalizeFeedbackMatchText(remoteMessage?.content);
  const localTime = resolveTimestampMs(localMessage?.created_at);
  const remoteTime = resolveTimestampMs(remoteMessage?.created_at);
  const hasTime = Number.isFinite(localTime) && Number.isFinite(remoteTime);
  const timeDelta = hasTime ? Math.abs(localTime - remoteTime) : Number.POSITIVE_INFINITY;

  let textScore = 0;
  if (localText && remoteText) {
    if (localText === remoteText) {
      textScore = 100000;
    } else if (localText.includes(remoteText) || remoteText.includes(localText)) {
      textScore = 80000;
    } else {
      return 0;
    }
  } else if (!localText && !remoteText) {
    if (!hasTime || timeDelta > 5000) {
      return 0;
    }
    textScore = 1000;
  } else {
    return 0;
  }

  let timeScore = 0;
  if (hasTime) {
    if (timeDelta <= 1000) {
      timeScore = 5000;
    } else if (timeDelta <= 10000) {
      timeScore = 4000;
    } else if (timeDelta <= 60000) {
      timeScore = 3000;
    } else if (timeDelta <= 180000) {
      timeScore = 1000;
    } else if (textScore < 100000) {
      return 0;
    }
  }
  return textScore + timeScore;
};

const applyAssistantHistoryIdBackfill = (messages, historyMessages) => {
  if (!Array.isArray(messages) || !Array.isArray(historyMessages)) {
    return 0;
  }
  const localCandidates = [];
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (!isAssistantFeedbackCandidate(message)) continue;
    localCandidates.push(message);
  }
  if (!localCandidates.length) {
    return 0;
  }

  const remoteCandidates = [];
  for (let index = historyMessages.length - 1; index >= 0; index -= 1) {
    const message = historyMessages[index];
    if (!message || message.role !== 'assistant' || message.isGreeting) continue;
    const historyId = resolveMessageHistoryId(message);
    if (historyId <= 0) continue;
    remoteCandidates.push(message);
  }
  if (!remoteCandidates.length) {
    return 0;
  }

  const usedHistoryIds = new Set();
  let updated = 0;
  for (const localMessage of localCandidates) {
    let bestMatch = null;
    let bestScore = 0;
    for (const remoteMessage of remoteCandidates) {
      const historyId = resolveMessageHistoryId(remoteMessage);
      if (historyId <= 0 || usedHistoryIds.has(historyId)) continue;
      const score = scoreAssistantHistoryMatch(localMessage, remoteMessage);
      if (score > bestScore) {
        bestScore = score;
        bestMatch = remoteMessage;
      }
    }
    if (!bestMatch || bestScore <= 0) continue;
    const historyId = resolveMessageHistoryId(bestMatch);
    if (historyId <= 0) continue;
    usedHistoryIds.add(historyId);
    localMessage.history_id = historyId;
    const feedback = normalizeMessageFeedback(bestMatch.feedback);
    if (feedback) {
      localMessage.feedback = {
        ...feedback,
        locked: true
      };
    }
    updated += 1;
  }
  return updated;
};

const applyMessageWindow = (store, sessionId, messages, options: { force?: boolean } = {}) => {
  if (!store || !isWindowingEnabled()) return;
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  const state = getHistoryState(key);
  const limit = Number(state.windowLimit) || MESSAGE_WINDOW_LIMIT;
  const threshold = Math.max(MESSAGE_WINDOW_THRESHOLD, limit);
  if (!options.force && messages.length <= threshold) return;
  if (messages.length <= limit) return;
  const overflow = messages.length - limit;
  if (overflow <= 0) return;
  messages.splice(0, overflow);
};

const applyHistoryMeta = (sessionId, detail, messages) => {
  const beforeId = Number.parseInt(
    String(
      detail?.history_before_id ??
        detail?.history_beforeId ??
        detail?.historyBeforeId ??
        ''
    ),
    10
  );
  const hasMore =
    detail?.history_has_more ??
    detail?.historyHasMore ??
    detail?.history_more ??
    detail?.historyMore ??
    null;
  const resolvedBeforeId =
    Number.isFinite(beforeId) && beforeId > 0 ? beforeId : findOldestHistoryId(messages);
  updateHistoryState(sessionId, {
    beforeId: resolvedBeforeId,
    hasMore: hasMore === null ? Boolean(resolvedBeforeId) : Boolean(hasMore)
  });
};

const cloneSerializable = (value, fallback) => {
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(value);
    } catch (error) {
      // Fallback to JSON clone when structuredClone fails.
    }
  }
  try {
    return JSON.parse(JSON.stringify(value));
  } catch (error) {
    return fallback;
  }
};

const cloneSessionList = (sessions) => {
  const cloned = cloneSerializable(Array.isArray(sessions) ? sessions : [], []);
  return Array.isArray(cloned) ? cloned : [];
};

const filterSessionsByAgent = (agentId, sourceSessions = []) => {
  const normalizedAgentId = String(agentId || '').trim();
  return (Array.isArray(sourceSessions) ? sourceSessions : []).filter((session) => {
    const sessionAgentId = String(session?.agent_id || '').trim();
    return normalizedAgentId ? sessionAgentId === normalizedAgentId : !sessionAgentId;
  });
};

const resolveInitialSessionIdFromList = (agentId, sourceSessions = []) => {
  const sessions = filterSessionsByAgent(agentId, sourceSessions);
  if (!sessions.length) return '';
  const mainSession = sessions.find((session) => session.is_main);
  if (mainSession?.id) {
    return mainSession.id;
  }
  const persistedSessionId = resolvePersistedSessionId(agentId);
  if (persistedSessionId && sessions.some((session) => session.id === persistedSessionId)) {
    return persistedSessionId;
  }
  return sessions[0]?.id || '';
};

const resolveSessionListCacheKey = (agentId) => normalizeAgentKey(agentId);

const readSessionListCache = (agentId) => {
  const cacheKey = resolveSessionListCacheKey(agentId);
  const cached = sessionListCache.get(cacheKey);
  if (!cached) return null;
  if (!Number.isFinite(cached.cachedAt) || Date.now() - cached.cachedAt > SESSION_LIST_CACHE_TTL_MS) {
    sessionListCache.delete(cacheKey);
    return null;
  }
  return cloneSessionList(cached.sessions);
};

const writeSessionListCache = (agentId, sessions) => {
  const cacheKey = resolveSessionListCacheKey(agentId);
  sessionListCache.set(cacheKey, {
    cachedAt: Date.now(),
    sessions: cloneSessionList(sessions)
  });
};

const normalizeThreadControlSession = (value) => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const session = value as Record<string, unknown>;
  const id = resolveSessionKey(session.id ?? session.session_id ?? session.sessionId);
  if (!id) return null;
  return {
    ...session,
    id
  } as ThreadControlSession;
};

const applyThreadControlSessionPatch = (store, session, options: { allowArchived?: boolean } = {}) => {
  const normalized = normalizeThreadControlSession(session);
  if (!normalized) return null;
  const targetId = resolveSessionKey(normalized.id);
  if (!targetId) return null;
  const status = String(normalized.status || '').trim().toLowerCase();
  const allowArchived = options.allowArchived === true;
  const targetAgentId = String(normalized.agent_id || '').trim();
  if (status === 'archived' && !allowArchived) {
    store.sessions = (Array.isArray(store.sessions) ? store.sessions : []).filter(
      (item) => resolveSessionKey(item?.id) !== targetId
    );
    if (resolvePersistedSessionId(targetAgentId) === targetId) {
      persistAgentSession(targetAgentId, '');
    }
    return { ...normalized, id: targetId };
  }
  const index = (Array.isArray(store.sessions) ? store.sessions : []).findIndex(
    (item) => resolveSessionKey(item?.id) === targetId
  );
  if (index >= 0) {
    const current = store.sessions[index] || {};
    store.sessions[index] = {
      ...current,
      ...normalized,
      id: targetId
    };
    return store.sessions[index];
  }
  const merged = { ...normalized, id: targetId };
  store.sessions.unshift(merged);
  return merged;
};

const applyThreadControlCaches = (store, agentIds: Set<string>) => {
  store.sessions = sortSessionsByActivity(store.sessions);
  agentIds.forEach((agentId) => {
    writeSessionListCache(agentId, filterSessionsByAgent(agentId, store.sessions));
  });
  syncDemoChatCache({ sessions: store.sessions });
};

const handleThreadControlWorkflowEvent = async (store, payloadRaw) => {
  const payload =
    payloadRaw && typeof payloadRaw === 'object' && !Array.isArray(payloadRaw)
      ? (payloadRaw as Record<string, unknown>)
      : {};
  const primarySession = normalizeThreadControlSession(payload.session);
  const mainSession = normalizeThreadControlSession(payload.main_session ?? payload.mainSession);
  const switchSession = normalizeThreadControlSession(
    payload.switch_session ?? payload.switchSession ?? payload.session
  );
  const activeSessionId = resolveSessionKey(store?.activeSessionId);
  const retainIds = new Set(
    [activeSessionId, resolveSessionKey(mainSession?.id), resolveSessionKey(switchSession?.id)].filter(Boolean)
  );
  const affectedAgentIds = new Set<string>();
  const applyPatch = (session, options: { allowArchived?: boolean } = {}) => {
    const patched = applyThreadControlSessionPatch(store, session, options);
    if (!patched) return null;
    affectedAgentIds.add(String(patched.agent_id || '').trim());
    return patched;
  };

  const patchedPrimary = applyPatch(primarySession, {
    allowArchived: retainIds.has(resolveSessionKey(primarySession?.id))
  });
  const patchedMain = applyPatch(mainSession, { allowArchived: true });
  const patchedSwitch = applyPatch(switchSession, { allowArchived: true });

  if (patchedMain?.id) {
    const mainAgentId = String(patchedMain.agent_id || '').trim();
    store.sessions = applyMainSession(store.sessions, mainAgentId, patchedMain.id);
    persistAgentSession(mainAgentId, patchedMain.id);
    affectedAgentIds.add(mainAgentId);
  }

  if (!patchedMain?.id && patchedPrimary?.status === 'archived') {
    const archivedAgentId = String(patchedPrimary.agent_id || '').trim();
    if (resolvePersistedSessionId(archivedAgentId) === patchedPrimary.id) {
      persistAgentSession(archivedAgentId, '');
    }
  }

  applyThreadControlCaches(store, affectedAgentIds);

  const shouldSwitch = payload.switch === true;
  const targetSwitchId = resolveSessionKey(
    patchedSwitch?.id ?? payload.switch_session_id ?? payload.switchSessionId ?? ''
  );
  if (shouldSwitch && targetSwitchId && targetSwitchId !== activeSessionId) {
    await store.loadSessionDetail(targetSwitchId);
  }
};

const resolveChatHttpStatus = (error) => {
  const status = Number(error?.response?.status ?? error?.status ?? 0);
  return Number.isFinite(status) ? status : 0;
};

const isSessionUnavailableStatus = (status) => [401, 403, 404].includes(Number(status || 0));

const hasKnownSessionInStore = (store, sessionId) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId) return false;
  const sessions = Array.isArray(store?.sessions) ? store.sessions : [];
  if (!sessions.length) return true;
  return sessions.some((item) => resolveSessionKey(item?.id) === targetId);
};

const purgeUnavailableSession = (store, sessionId) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId) return '';
  const sessions = Array.isArray(store?.sessions) ? store.sessions : [];
  const targetSession = sessions.find((item) => resolveSessionKey(item?.id) === targetId) || null;
  const targetAgentId = String(targetSession?.agent_id || '').trim();
  abortResumeStream(targetId);
  abortSendStream(targetId);
  abortWatchStream(targetId);
  if (resolveSessionKey(store?.activeSessionId) === targetId) {
    clearSessionWatcher();
  }
  setSessionLoading(store, targetId, false);
  if (typeof store?.clearPendingApprovals === 'function') {
    store.clearPendingApprovals({ sessionId: targetId });
  }
  sessionRuntime.delete(targetId);
  sessionMessages.delete(targetId);
  sessionDetailWarmState.delete(targetId);
  sessionDetailPrefetchInFlight.delete(targetId);
  sessionSubagentsInFlight.delete(targetId);
  sessionHistoryState.delete(targetId);
  sessionWorkflowState.delete(targetId);
  clearSessionCommandSessions(targetId);
  removeDemoChatSession(targetId);
  clearChatSnapshot(targetId);

  const nextSessions = sessions.filter((item) => resolveSessionKey(item?.id) !== targetId);
  if (Array.isArray(store?.sessions)) {
    store.sessions = nextSessions;
  }
  if (resolvePersistedSessionId(targetAgentId) === targetId) {
    persistAgentSession(targetAgentId, '');
  }
  writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, nextSessions));

  if (resolveSessionKey(store?.activeSessionId) === targetId) {
    store.activeSessionId = null;
    store.draftAgentId = targetAgentId;
    store.draftToolOverrides = null;
    store.messages = ensureGreetingMessage([], {
      greeting: store?.greetingOverride
    });
    persistDraftSession();
  }
  syncDemoChatCache({
    sessions: Array.isArray(store?.sessions) ? store.sessions : nextSessions,
    sessionId: store?.activeSessionId || null,
    messages: Array.isArray(store?.messages) ? store.messages : []
  });
  return targetAgentId;
};

const markSessionDetailWarm = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return;
  sessionDetailWarmState.set(sessionKey, Date.now() + SESSION_DETAIL_WARM_TTL_MS);
};

const isSessionDetailWarm = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return false;
  const warmUntil = Number(sessionDetailWarmState.get(sessionKey));
  if (!Number.isFinite(warmUntil)) {
    sessionDetailWarmState.delete(sessionKey);
    return false;
  }
  if (warmUntil <= Date.now()) {
    sessionDetailWarmState.delete(sessionKey);
    return false;
  }
  return true;
};

const ensureRuntime = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  if (!sessionRuntime.has(key)) {
    sessionRuntime.set(key, {
      sendController: null,
      compactController: null,
      resumeController: null,
      sendRequestId: null,
      resumeRequestId: null,
      watchController: null,
      watchRequestId: null,
      watchLastEventAt: 0,
      watchdogTimer: null,
      watchdogBusy: false,
      watchReconcileTimer: null,
      watchReconcileAt: 0,
      slowClientResumeTimer: null,
      slowClientResumeAfterEventId: 0,
      stopRequested: false,
      lastEventId: 0,
      threadStatus: 'not_loaded',
      loaded: false,
      activeTurnId: '',
      pendingApprovalIds: [],
      pendingApprovalCount: 0,
      waitingForUserInput: false,
      lastThreadStatusAt: 0
    });
  }
  return sessionRuntime.get(key);
};

const getRuntime = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  return sessionRuntime.get(key) || null;
};

function resolveRuntimeSessionId(sessionId, payload) {
  const direct = resolveSessionKey(sessionId ?? payload?.session_id ?? payload?.sessionId);
  if (direct) return direct;
  const threadId = String(payload?.thread_id ?? payload?.threadId ?? '').trim();
  if (!threadId.startsWith('thread_')) return null;
  return resolveSessionKey(threadId.slice('thread_'.length));
}

function normalizeRuntimeApprovalIds(value) {
  if (!Array.isArray(value)) return [];
  return Array.from(
    new Set(
      value
        .map((item) => String(item || '').trim())
        .filter(Boolean)
    )
  );
}

function resolveRuntimeLoading(store, sessionId, runtime) {
  const key = resolveSessionKey(sessionId);
  if (!key) return false;
  if (Boolean(store?.loadingBySession?.[key])) {
    return true;
  }
  return hasRuntimeControllers(runtime);
}

function hasRuntimeControllers(runtime) {
  if (!runtime) return false;
  return Boolean(runtime?.sendController || runtime?.resumeController || runtime?.compactController);
}

function applyRuntimeDerivedStatus(store, sessionId, runtime) {
  if (!runtime) return 'not_loaded';
  if (runtime.waitingForUserInput) {
    runtime.threadStatus = 'waiting_user_input';
    runtime.loaded = true;
    return runtime.threadStatus;
  }
  if (Number(runtime.pendingApprovalCount) > 0) {
    runtime.threadStatus = 'waiting_approval';
    runtime.loaded = true;
    return runtime.threadStatus;
  }
  const loading = resolveRuntimeLoading(store, sessionId, runtime);
  const current = normalizeThreadRuntimeStatus(runtime.threadStatus);
  if (loading) {
    runtime.threadStatus = 'running';
    runtime.loaded = true;
    return runtime.threadStatus;
  }
  if (current === 'system_error') {
    return current;
  }
  runtime.threadStatus = runtime.loaded ? 'idle' : 'not_loaded';
  return runtime.threadStatus;
}

function applySessionRuntimeSnapshot(runtime, snapshot) {
  if (!runtime || !snapshot || typeof snapshot !== 'object' || Array.isArray(snapshot)) {
    return false;
  }
  const source = snapshot as Record<string, unknown>;
  const turn =
    source.turn && typeof source.turn === 'object' && !Array.isArray(source.turn)
      ? (source.turn as Record<string, unknown>)
      : {};
  const pendingApprovalIds = normalizeRuntimeApprovalIds(
    turn.pending_approval_ids ?? turn.pendingApprovalIds
  );
  const waitingForUserInput = normalizeFlag(
    turn.waiting_for_user_input ?? turn.waitingForUserInput
  );
  const explicitApprovalCount = Number.parseInt(
    String(turn.pending_approval_count ?? turn.pendingApprovalCount ?? ''),
    10
  );
  runtime.loaded =
    source.loaded === undefined
      ? runtime.loaded || normalizeThreadRuntimeStatus(source.thread_status ?? source.status) !== 'not_loaded'
      : normalizeFlag(source.loaded);
  runtime.activeTurnId = String(
    source.active_turn_id ?? source.activeTurnId ?? turn.turn_id ?? turn.turnId ?? ''
  ).trim();
  runtime.pendingApprovalIds = pendingApprovalIds;
  runtime.pendingApprovalCount =
    Number.isFinite(explicitApprovalCount) && explicitApprovalCount >= 0
      ? explicitApprovalCount
      : pendingApprovalIds.length;
  runtime.waitingForUserInput = waitingForUserInput;
  runtime.threadStatus = normalizeThreadRuntimeStatus(source.thread_status ?? source.status);
  runtime.lastThreadStatusAt = Date.now();
  if (runtime.waitingForUserInput) {
    runtime.threadStatus = 'waiting_user_input';
    runtime.loaded = true;
  } else if (runtime.pendingApprovalCount > 0) {
    runtime.threadStatus = 'waiting_approval';
    runtime.loaded = true;
  } else if (runtime.threadStatus === 'not_loaded') {
    runtime.loaded = false;
    runtime.activeTurnId = '';
  }
  return true;
}

function applySessionRuntimeEvent(store, sessionId, payload, eventType = 'thread_status') {
  const targetId = resolveRuntimeSessionId(sessionId, payload);
  if (!targetId) return null;
  const runtime = ensureRuntime(targetId);
  if (!runtime) return null;
  const applied = applySessionRuntimeSnapshot(runtime, payload);
  if (!applied && eventType === 'thread_closed') {
    runtime.loaded = false;
    runtime.activeTurnId = '';
    runtime.pendingApprovalIds = [];
    runtime.pendingApprovalCount = 0;
    runtime.waitingForUserInput = false;
    runtime.threadStatus = 'not_loaded';
    runtime.lastThreadStatusAt = Date.now();
  } else if (applied && eventType === 'thread_closed') {
    runtime.loaded = false;
    runtime.activeTurnId = '';
    runtime.pendingApprovalIds = [];
    runtime.pendingApprovalCount = 0;
    runtime.waitingForUserInput = false;
    runtime.threadStatus = 'not_loaded';
  }
  const normalizedStatus = normalizeThreadRuntimeStatus(runtime.threadStatus);
  const shouldSettleTerminalState =
    !hasRuntimeControllers(runtime)
    && normalizedStatus !== 'running'
    && !isThreadRuntimeWaiting(normalizedStatus);
  if (shouldSettleTerminalState) {
    const targetMessages = resolveSessionKey(store?.activeSessionId) === targetId
      ? store?.messages
      : getSessionMessages(targetId);
    const clearedSuperseded = clearSupersededPendingAssistantMessages(targetMessages);
    const clearedTrailing = stopPendingAssistantMessage(findPendingAssistantMessage(targetMessages));
    if (clearedSuperseded || clearedTrailing) {
      notifySessionSnapshot(store, targetId, targetMessages, true);
    }
    setSessionLoading(store, targetId, false);
  }
  return runtime;
}

function syncSessionPendingApprovalRuntime(store, sessionId) {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  const runtime = ensureRuntime(key);
  if (!runtime) return null;
  const approvals = Array.isArray(store?.pendingApprovals)
    ? store.pendingApprovals.filter((item) => resolveSessionKey(item?.session_id) === key)
    : [];
  runtime.pendingApprovalIds = approvals
    .map((item) => String(item?.approval_id || '').trim())
    .filter(Boolean);
  runtime.pendingApprovalCount = runtime.pendingApprovalIds.length;
  applyRuntimeDerivedStatus(store, key, runtime);
  return runtime;
}

const getSessionMessages = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  return sessionMessages.get(key) || null;
};

const cacheSessionMessages = (sessionId, messages) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  dedupeAssistantMessagesInPlace(messages);
  sessionMessages.set(key, messages);
};

const hasSubmittedUserMessage = (messages) =>
  (Array.isArray(messages) ? messages : []).some((message) => {
    if (!message || message.isGreeting || String(message.role || '').trim() !== 'user') {
      return false;
    }
    const hasText = Boolean(String(message.content || '').trim());
    const hasAttachments = Array.isArray(message.attachments) && message.attachments.length > 0;
    return hasText || hasAttachments;
  });

const isReusableFreshSession = (session, fallbackMessages = null) => {
  if (!session || typeof session !== 'object') return false;
  const sessionId = resolveSessionKey(session.id);
  if (!sessionId) return false;
  const status = String(session.status || '').trim().toLowerCase();
  if (status === 'archived') return false;
  const cachedMessages = getSessionMessages(sessionId);
  const messages = Array.isArray(cachedMessages) && cachedMessages.length ? cachedMessages : fallbackMessages;
  if (hasSubmittedUserMessage(messages)) {
    return false;
  }
  const createdAt = resolveTimestampMs(session.created_at);
  const lastMessageAt = resolveTimestampMs(session.last_message_at);
  if (createdAt !== null && lastMessageAt !== null && lastMessageAt > createdAt + 1000) {
    return false;
  }
  return true;
};

const touchSessionUpdatedAt = (store, sessionId, timestamp) => {
  if (!store || !Array.isArray(store.sessions)) return;
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  const session = store.sessions.find((item) => String(item?.id || '').trim() === key);
  if (!session) return;
  const resolved = resolveTimestampIso(timestamp);
  session.updated_at = resolved || new Date().toISOString();
};

const syncSessionContextTokens = (store, sessionId, contextTokens, contextTotalTokens = null) => {
  if (!store || !Array.isArray(store.sessions)) return;
  const key = resolveSessionKey(sessionId);
  const normalized = parseOptionalCount(contextTokens);
  const normalizedTotal = normalizeContextTotalTokens(contextTotalTokens);
  if (!key || normalized === null) return;
  const index = store.sessions.findIndex((item) => resolveSessionKey(item?.id) === key);
  if (index < 0) return;
  const current = store.sessions[index] || {};
  const next = {
    ...current,
    context_tokens: normalized,
    ...(normalizedTotal !== null ? { context_max_tokens: normalizedTotal } : {})
  };
  store.sessions[index] = next;
  const agentId = String(next.agent_id || '').trim();
  writeSessionListCache(agentId, filterSessionsByAgent(agentId, store.sessions));
  syncDemoChatCache({ sessions: store.sessions });
};

const notifySessionSnapshot = (store, sessionId, messages, immediate = false, options: { skipWindowing?: boolean } = {}) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  cacheSessionMessages(key, messages);
  const activeKey = resolveSessionKey(store?.activeSessionId);
  if (activeKey && activeKey === key) {
    if (options.skipWindowing !== true) {
      applyMessageWindow(store, key, messages);
    }
    scheduleChatSnapshot(store, immediate);
  }
};

const shouldPreferCachedMessages = (cached, server) => {
  if (!Array.isArray(cached) || cached.length === 0) return false;
  if (!Array.isArray(server) || server.length === 0) return true;
  if (cached.some((message) => message?.stream_incomplete || message?.workflowStreaming)) {
    return true;
  }
  const cachedLastAssistant = [...cached].reverse().find((message) => message?.role === 'assistant');
  const serverLastAssistant = [...server].reverse().find((message) => message?.role === 'assistant');
  if (cachedLastAssistant || serverLastAssistant) {
    const cachedEventId = normalizeStreamEventId(cachedLastAssistant?.stream_event_id);
    const serverEventId = normalizeStreamEventId(serverLastAssistant?.stream_event_id);
    if (cachedEventId !== null && (serverEventId === null || cachedEventId > serverEventId)) {
      return true;
    }
    const cachedContentLen = String(cachedLastAssistant?.content || '').length;
    const serverContentLen = String(serverLastAssistant?.content || '').length;
    if (cachedContentLen > serverContentLen) {
      return true;
    }
  }
  return cached.length > server.length;
};

const clearCompletedAssistantStreamingState = (messages) => {
  if (!Array.isArray(messages)) return;
  messages.forEach((message) => {
    if (!message || message.role !== 'assistant') return;
    message.workflowStreaming = false;
    message.stream_incomplete = false;
    message.reasoningStreaming = false;
  });
};

const setSessionLoading = (store, sessionId, value) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  if (value) {
    store.loadingBySession[key] = true;
  } else if (store.loadingBySession[key]) {
    delete store.loadingBySession[key];
  }
  const runtime = ensureRuntime(key);
  if (!runtime) return;
  if (value) {
    runtime.loaded = true;
    if (!isThreadRuntimeWaiting(runtime.threadStatus)) {
      runtime.threadStatus = 'running';
    }
    return;
  }
  applyRuntimeDerivedStatus(store, key, runtime);
};

let sessionWatchSessionId = '';

const clearWatchdog = (runtime) => {
  if (!runtime) return;
  if (runtime.watchdogTimer) {
    clearTimeout(runtime.watchdogTimer);
    runtime.watchdogTimer = null;
  }
  if (runtime.watchReconcileTimer) {
    clearTimeout(runtime.watchReconcileTimer);
    runtime.watchReconcileTimer = null;
  }
  runtime.watchReconcileAt = 0;
  runtime.watchdogBusy = false;
  runtime.watchLastEventAt = 0;
};

const clearSlowClientResume = (runtime) => {
  if (!runtime) return;
  if (runtime.slowClientResumeTimer) {
    clearTimeout(runtime.slowClientResumeTimer);
    runtime.slowClientResumeTimer = null;
  }
  runtime.slowClientResumeAfterEventId = 0;
};

const abortWatchStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  if (runtime.watchController) {
    runtime.watchController.abort();
    runtime.watchController = null;
  }
  runtime.watchRequestId = null;
  clearWatchdog(runtime);
};

const clearSessionWatcher = () => {
  if (sessionWatchSessionId) {
    abortWatchStream(sessionWatchSessionId);
  }
  sessionWatchSessionId = '';
};

const resolveMaxStreamEventId = (messages) => {
  if (!Array.isArray(messages)) return null;
  let maxId = 0;
  messages.forEach((message) => {
    const eventId = normalizeStreamEventId(message.stream_event_id);
    if (eventId && eventId > maxId) {
      maxId = eventId;
    }
  });
  return maxId > 0 ? maxId : null;
};

const resolveLastStreamEventId = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    const eventId = normalizeStreamEventId(message?.stream_event_id);
    if (eventId !== null) {
      return eventId;
    }
  }
  return null;
};

const resolveLastAssistantStreamEventId = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    const eventId = normalizeStreamEventId(message.stream_event_id);
    if (eventId !== null) {
      return eventId;
    }
  }
  return null;
};

const resolveMaxStreamRound = (messages) => {
  if (!Array.isArray(messages)) return null;
  let maxRound = 0;
  messages.forEach((message) => {
    if (message?.role !== 'assistant') return;
    const round = normalizeStreamRound(message.stream_round);
    if (round && round > maxRound) {
      maxRound = round;
    }
  });
  return maxRound > 0 ? maxRound : null;
};

const findAssistantMessageByUserRound = (messages, userRound) => {
  const normalizedRound = normalizeStreamRound(userRound);
  if (!Array.isArray(messages) || normalizedRound === null || normalizedRound <= 0) return null;
  let userCount = 0;
  for (let i = 0; i < messages.length; i += 1) {
    const message = messages[i];
    if (message?.role !== 'user') continue;
    userCount += 1;
    if (userCount !== normalizedRound) continue;
    for (let j = i + 1; j < messages.length; j += 1) {
      const candidate = messages[j];
      if (candidate?.role === 'assistant') return candidate;
      if (candidate?.role === 'user') return null;
    }
    return null;
  }
  return null;
};

const findAssistantMessageByRound = (messages, roundNumber) => {
  if (!Array.isArray(messages) || !Number.isFinite(roundNumber) || roundNumber <= 0) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    if (normalizeStreamRound(message.stream_round) === roundNumber) {
      return message;
    }
  }
  return null;
};

const ensurePendingAssistantMessage = (store, sessionId, messages, baseEventId) => {
  if (!Array.isArray(messages)) return null;
  const existing = findPendingAssistantMessage(messages);
  if (existing) return existing;
  const placeholder = {
    ...buildMessage('assistant', ''),
    workflowItems: [],
    workflowStreaming: false,
    stream_incomplete: true,
    stream_event_id: baseEventId || 0,
    stream_round: null
  };
  messages.push(placeholder);
  notifySessionSnapshot(store, sessionId, messages, true);
  return placeholder;
};

const resolveLastAssistantTimestampMs = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    const timestamp = resolveTimestampMs(message.created_at);
    return Number.isFinite(timestamp) ? timestamp : null;
  }
  return null;
};

const WATCH_USER_MESSAGE_DEDUP_MS = 2000;
const WATCHDOG_IDLE_MS_ACTIVE = 1500;
const WATCHDOG_IDLE_MS_BACKGROUND = 14000;
const WATCHDOG_IDLE_MS_HIDDEN = 26000;
const WATCHDOG_INTERVAL_MS_ACTIVE = 500;
const WATCHDOG_INTERVAL_MS_BACKGROUND = 3500;
const WATCHDOG_INTERVAL_MS_HIDDEN = 7000;
const WATCH_RECONCILE_DELAY_MS = 150;
const WATCH_RECONCILE_COOLDOWN_MS = 1800;
const SLOW_CLIENT_RESUME_DELAY_MS = 120;
const STREAM_FLUSH_BASE_MS = 20;
const STREAM_FLUSH_MAX_MS = 160;
const HISTORY_PAGE_LIMIT = 80;
const HISTORY_PAGE_MAX = 200;
const MESSAGE_WINDOW_LIMIT = 400;
const MESSAGE_WINDOW_THRESHOLD = 600;
const MESSAGE_WINDOW_MAX = 2000;
const WINDOWING_ENABLED_KEY = 'wunder_chat_windowing';

const resolveStreamFlushMs = (messageCount, override) => {
  if (Number.isFinite(override)) {
    return Math.max(0, Number(override));
  }
  const count = Number.isFinite(messageCount) ? Number(messageCount) : 0;
  if (count > 1000) return STREAM_FLUSH_MAX_MS;
  if (count > 500) return 120;
  if (count > 200) return 80;
  return STREAM_FLUSH_BASE_MS;
};

const resolveStreamFlushMsForMessages = (messages) =>
  resolveStreamFlushMs(Array.isArray(messages) ? messages.length : 0, null);

const normalizeHistoryPageLimit = (value) => {
  const parsed = Number.parseInt(String(value ?? HISTORY_PAGE_LIMIT), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) return HISTORY_PAGE_LIMIT;
  return Math.min(parsed, HISTORY_PAGE_MAX);
};

const isWindowingEnabled = () => {
  try {
    const raw = localStorage.getItem(WINDOWING_ENABLED_KEY);
    if (!raw) return true;
    return raw !== '0' && raw.toLowerCase() !== 'false';
  } catch (error) {
    return true;
  }
};

const shouldInsertWatchUserMessage = (messages, content, eventTimestampMs) => {
  if (!Array.isArray(messages) || !content) return false;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'user') continue;
    const lastContent = String(message.content || '');
    if (lastContent !== content) {
      return true;
    }
    const lastTimestamp = resolveTimestampMs(message.created_at);
    if (!Number.isFinite(eventTimestampMs) || !Number.isFinite(lastTimestamp)) {
      return false;
    }
    return Math.abs(eventTimestampMs - lastTimestamp) > WATCH_USER_MESSAGE_DEDUP_MS;
  }
  return true;
};

const isDocumentHidden = () =>
  typeof document !== 'undefined' && document.visibilityState === 'hidden';

const resolveWatchdogProfile = (store, sessionId) => {
  if (isDocumentHidden()) {
    return {
      idleMs: WATCHDOG_IDLE_MS_HIDDEN,
      intervalMs: WATCHDOG_INTERVAL_MS_HIDDEN
    };
  }
  const activeSessionId = resolveSessionKey(store?.activeSessionId);
  const isForeground = Boolean(activeSessionId && activeSessionId === sessionId);
  if (isForeground) {
    return {
      idleMs: WATCHDOG_IDLE_MS_ACTIVE,
      intervalMs: WATCHDOG_INTERVAL_MS_ACTIVE
    };
  }
  return {
    idleMs: WATCHDOG_IDLE_MS_BACKGROUND,
    intervalMs: WATCHDOG_INTERVAL_MS_BACKGROUND
  };
};

const hasAnchoredWatchUserMessage = (messages, anchor, content) => {
  if (!Array.isArray(messages) || !anchor || !content) return false;
  const anchorIndex = messages.indexOf(anchor);
  if (anchorIndex <= 0) return false;
  const previous = messages[anchorIndex - 1];
  if (previous?.role !== 'user') return false;
  return String(previous.content || '') === content;
};

const insertWatchUserMessage = (
  store,
  sessionId,
  messages,
  content,
  eventTimestampMs,
  anchor,
  options = {}
) => {
  const optionRecord: Record<string, unknown> =
    options && typeof options === 'object' ? (options as Record<string, unknown>) : {};
  if (hasAnchoredWatchUserMessage(messages, anchor, content)) {
    return;
  }
  if (!shouldInsertWatchUserMessage(messages, content, eventTimestampMs)) {
    return;
  }
  const createdAt = Number.isFinite(eventTimestampMs)
    ? new Date(eventTimestampMs).toISOString()
    : undefined;
  const userMessage = buildMessage('user', content, createdAt, {
    hiddenInternal: normalizeHiddenInternalMessage(optionRecord.hiddenInternal)
  });
  const anchorIndex = anchor ? messages.indexOf(anchor) : -1;
  if (anchorIndex >= 0) {
    messages.splice(anchorIndex, 0, userMessage);
  } else {
    messages.push(userMessage);
  }
  clearSupersededPendingAssistantMessages(messages);
  dismissStaleInquiryPanels(messages);
  touchSessionUpdatedAt(store, sessionId, eventTimestampMs ?? Date.now());
  notifySessionSnapshot(store, sessionId, messages, true);
};

const startSessionWatcher = (store, sessionId) => {
  clearSessionWatcher();
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  if (!hasKnownSessionInStore(store, key)) {
    purgeUnavailableSession(store, key);
    return;
  }
  sessionWatchSessionId = key;
  const runtime = ensureRuntime(key);
  if (!runtime) return;
  if (runtime.sendController?.signal?.aborted) {
    runtime.sendController = null;
  }
  if (runtime.resumeController?.signal?.aborted) {
    runtime.resumeController = null;
  }
  if (runtime.sendController || runtime.resumeController) return;
  const perfEnabled = chatPerf.enabled();
  runtime.watchController = new AbortController();
  const controller = runtime.watchController;
  runtime.watchLastEventAt = Date.now();
  runtime.watchReconcileAt = 0;
  const requestId = buildWsRequestId();
  runtime.watchRequestId = requestId;
  const sessionMessagesRef = getSessionMessages(key) || store.messages;
  cacheSessionMessages(key, sessionMessagesRef);
  const workflowState = getSessionWorkflowState(key);
  const roundStates = new Map();
  const completedRounds = new Set();
  let maxKnownRound = resolveMaxStreamRound(sessionMessagesRef) || 0;
  const tailEventId =
    resolveLastStreamEventId(sessionMessagesRef) ||
    resolveLastAssistantStreamEventId(sessionMessagesRef) ||
    resolveMaxStreamEventId(sessionMessagesRef) ||
    0;
  const runtimeLastEventId = getRuntimeLastEventId(runtime);
  let lastEventId = runtimeLastEventId > 0 ? runtimeLastEventId : tailEventId;
  const minEventTimestampMs =
    lastEventId > 0 ? null : resolveLastAssistantTimestampMs(sessionMessagesRef);

  const ensureRoundState = (
    roundNumber,
    eventTimestampMs,
    userRoundNumber = null,
    options: { preferFreshRound?: boolean } = {}
  ) => {
    const normalizedRound = normalizeStreamRound(roundNumber);
    if (normalizedRound === null || normalizedRound <= 0) return null;
    maxKnownRound = Math.max(maxKnownRound, normalizedRound);
    if (completedRounds.has(normalizedRound)) return null;
    const existing = roundStates.get(normalizedRound);
    if (existing) return existing;
    const normalizedUserRound = normalizeStreamRound(userRoundNumber);
    const candidateByUserRound = normalizedUserRound
      ? findAssistantMessageByUserRound(sessionMessagesRef, normalizedUserRound)
      : null;
    const pendingCandidate = findPendingAssistantMessage(sessionMessagesRef);
    const pendingRound = normalizeStreamRound(pendingCandidate?.stream_round);
    const pendingCreatedAtMs = resolveTimestampMs(pendingCandidate?.created_at);
    const pendingHasContent =
      typeof pendingCandidate?.content === 'string' && pendingCandidate.content.trim().length > 0;
    const pendingHasWorkflow =
      Array.isArray(pendingCandidate?.workflowItems) && pendingCandidate.workflowItems.length > 0;
    const pendingRoundMismatch =
      pendingRound !== null &&
      pendingRound !== normalizedRound &&
      (normalizedUserRound === null || pendingRound !== normalizedUserRound);
    const preferFreshRound = options?.preferFreshRound === true;
    const pendingNeedsReset =
      Boolean(pendingCandidate) &&
      preferFreshRound &&
      (pendingHasContent || pendingHasWorkflow) &&
      (pendingRound === null || pendingRoundMismatch);
    if (pendingNeedsReset && stopPendingAssistantMessage(pendingCandidate)) {
      const panel = normalizeInquiryPanelState(pendingCandidate?.questionPanel);
      if (panel?.status === 'pending') {
        pendingCandidate.questionPanel = { ...panel, status: 'dismissed' };
      }
    }
    const pendingLooksStale =
      Boolean(pendingCandidate) &&
      Number.isFinite(eventTimestampMs) &&
      Number.isFinite(pendingCreatedAtMs) &&
      eventTimestampMs > Number(pendingCreatedAtMs) + 1500 &&
      (pendingHasContent || pendingHasWorkflow);
    const reusablePending =
      pendingCandidate && !pendingNeedsReset && !pendingRoundMismatch && !pendingLooksStale
        ? pendingCandidate
        : null;
    const candidate =
      candidateByUserRound ||
      reusablePending ||
      findAssistantMessageByRound(sessionMessagesRef, normalizedRound);
    if (candidate) {
      const assignedRound = normalizeStreamRound(candidate.stream_round);
      const alreadyTracked = Array.from(roundStates.values()).find((entry) => entry.message === candidate);
      if (alreadyTracked) {
        if (!roundStates.has(normalizedRound)) {
          roundStates.set(normalizedRound, alreadyTracked);
        }
        if (assignedRound === null || assignedRound !== normalizedRound) {
          candidate.stream_round = normalizedRound;
        }
        if (!candidate.created_at && Number.isFinite(eventTimestampMs)) {
          candidate.created_at = new Date(eventTimestampMs).toISOString();
        }
        candidate.workflowStreaming = true;
        candidate.stream_incomplete = true;
        return alreadyTracked;
      }
      const candidatePending =
        normalizeFlag(candidate.stream_incomplete) || normalizeFlag(candidate.workflowStreaming);
      const candidateHasContent =
        typeof candidate.content === 'string' && candidate.content.trim().length > 0;
      const candidateHasWorkflow =
        Array.isArray(candidate.workflowItems) && candidate.workflowItems.length > 0;
      const placeholderCandidate =
        assignedRound === normalizedRound &&
        !candidatePending &&
        !candidateHasContent &&
        !candidateHasWorkflow;
      if (
        candidatePending ||
        assignedRound === null ||
        (normalizedUserRound !== null && assignedRound === normalizedUserRound) ||
        Boolean(candidateByUserRound) ||
        placeholderCandidate
      ) {
        if (assignedRound === null || assignedRound !== normalizedRound) {
          candidate.stream_round = normalizedRound;
        }
        if (!candidate.created_at && Number.isFinite(eventTimestampMs)) {
          candidate.created_at = new Date(eventTimestampMs).toISOString();
        }
        candidate.workflowStreaming = true;
        candidate.stream_incomplete = true;
        const processor = createWorkflowProcessor(
          candidate,
          workflowState,
          () => notifySessionSnapshot(store, key, sessionMessagesRef),
          {
            streamFlushMs: resolveStreamFlushMsForMessages(sessionMessagesRef),
            sessionId: key,
            onThreadControl: (payload) => handleThreadControlWorkflowEvent(store, payload),
            onContextUsage: (contextTokens, contextTotalTokens) =>
              syncSessionContextTokens(store, key, contextTokens, contextTotalTokens)
          }
        );
        const state = { message: candidate, processor, userInserted: false };
        roundStates.set(normalizedRound, state);
        return state;
      }
    }
    const createdAt = Number.isFinite(eventTimestampMs)
      ? new Date(eventTimestampMs).toISOString()
      : undefined;
    const assistantMessage = {
      ...buildMessage('assistant', '', createdAt),
      workflowItems: [],
      workflowStreaming: true,
      stream_incomplete: true,
      stream_event_id: lastEventId || 0,
      stream_round: normalizedRound
    };
    sessionMessagesRef.push(assistantMessage);
    notifySessionSnapshot(store, key, sessionMessagesRef, true);
    const processor = createWorkflowProcessor(
      assistantMessage,
      workflowState,
      () => notifySessionSnapshot(store, key, sessionMessagesRef),
      {
        streamFlushMs: resolveStreamFlushMsForMessages(sessionMessagesRef),
        sessionId: key,
        onThreadControl: (payload) => handleThreadControlWorkflowEvent(store, payload),
        onContextUsage: (contextTokens, contextTotalTokens) =>
          syncSessionContextTokens(store, key, contextTokens, contextTotalTokens)
      }
    );
    const state = { message: assistantMessage, processor, userInserted: false };
    roundStates.set(normalizedRound, state);
    return state;
  };

  const finalizeRound = (roundNumber, aborted) => {
    const normalizedRound = normalizeStreamRound(roundNumber);
    if (normalizedRound === null) return;
    maxKnownRound = Math.max(maxKnownRound, normalizedRound);
    const state = roundStates.get(normalizedRound);
    if (!state) return;
    if (!aborted) {
      state.message.stream_incomplete = false;
      state.message.workflowStreaming = false;
    }
    state.processor.finalize();
    notifySessionSnapshot(store, key, sessionMessagesRef, true);
    const messageRef = state.message;
    Array.from(roundStates.entries()).forEach(([roundKey, entry]) => {
      if (entry.message === messageRef) {
        roundStates.delete(roundKey);
        completedRounds.add(roundKey);
      }
    });
  };

  const finalizeAll = (aborted) => {
    Array.from(roundStates.keys()).forEach((round) => finalizeRound(round, aborted));
  };

  const resolveWatchRoundNumber = (
    eventType,
    payload,
    data,
    isRoundStart
  ) => {
    const directRound = resolveEventRoundNumber(payload, data);
    if (directRound !== null) {
      maxKnownRound = Math.max(maxKnownRound, directRound);
      return directRound;
    }

    const activeRound = Array.from(roundStates.keys()).reduce(
      (maxValue, value) => (value > maxValue ? value : maxValue),
      0
    );
    if (isRoundStart) {
      const nextRound = activeRound > 0 ? activeRound + 1 : Math.max(maxKnownRound + 1, 1);
      maxKnownRound = Math.max(maxKnownRound, nextRound);
      return nextRound;
    }

    const segmented = parseSegmentedDelta(payload, data);
    const hasNonEmptyText = (value: unknown) =>
      typeof value === 'string' && value.trim().length > 0;
    const hasContentDelta =
      Boolean(segmented?.delta) ||
      Boolean(segmented?.reasoningDelta) ||
      hasNonEmptyText(data?.delta) ||
      hasNonEmptyText(data?.content) ||
      hasNonEmptyText(data?.answer) ||
      hasNonEmptyText(data?.message) ||
      hasNonEmptyText(payload?.message);
    const normalizedEventType = String(eventType || '').trim().toLowerCase();
    const eventHasWorkflowHint =
      hasContentDelta ||
      normalizedEventType === 'final' ||
      normalizedEventType === 'error' ||
      normalizedEventType === 'queue_enter' ||
      normalizedEventType === 'queue_start' ||
      normalizedEventType === 'queue_finish' ||
      normalizedEventType === 'queue_fail' ||
      normalizedEventType === 'received' ||
      normalizedEventType === 'round_start' ||
      normalizedEventType === 'progress' ||
      normalizedEventType === 'message' ||
      normalizedEventType === 'delta' ||
      normalizedEventType === 'think_delta' ||
      normalizedEventType === 'reasoning_delta' ||
      normalizedEventType === 'llm_output' ||
      normalizedEventType === 'tool_call' ||
      normalizedEventType === 'tool_result' ||
      normalizedEventType === 'tool_output' ||
      normalizedEventType === 'team_start' ||
      normalizedEventType === 'team_task_dispatch' ||
      normalizedEventType === 'team_task_update' ||
      normalizedEventType === 'team_task_result' ||
      normalizedEventType === 'team_merge' ||
      normalizedEventType === 'team_progress' ||
      normalizedEventType === 'team_finish' ||
      normalizedEventType === 'team_error' ||
      normalizedEventType.startsWith('subagent_');
    if (!eventHasWorkflowHint) {
      return null;
    }

    if (activeRound > 0) {
      maxKnownRound = Math.max(maxKnownRound, activeRound);
      return activeRound;
    }
    const fallbackRound = Math.max(maxKnownRound, 1);
    maxKnownRound = Math.max(maxKnownRound, fallbackRound);
    return fallbackRound;
  };

  const isWatchWorkflowEventType = (normalizedEventType) =>
    normalizedEventType === 'final' ||
    normalizedEventType === 'error' ||
    normalizedEventType === 'queue_enter' ||
    normalizedEventType === 'queue_start' ||
    normalizedEventType === 'queue_finish' ||
    normalizedEventType === 'queue_fail' ||
    normalizedEventType === 'received' ||
    normalizedEventType === 'round_start' ||
    normalizedEventType === 'progress' ||
    normalizedEventType === 'message' ||
    normalizedEventType === 'delta' ||
    normalizedEventType === 'think_delta' ||
    normalizedEventType === 'reasoning_delta' ||
    normalizedEventType === 'llm_output' ||
    normalizedEventType === 'tool_call' ||
    normalizedEventType === 'tool_result' ||
    normalizedEventType === 'tool_output' ||
    normalizedEventType === 'team_start' ||
    normalizedEventType === 'team_task_dispatch' ||
    normalizedEventType === 'team_task_update' ||
    normalizedEventType === 'team_task_result' ||
    normalizedEventType === 'team_merge' ||
    normalizedEventType === 'team_progress' ||
    normalizedEventType === 'team_finish' ||
    normalizedEventType === 'team_error' ||
    normalizedEventType.startsWith('subagent_');

  const isWatchSidebandEventType = (normalizedEventType) =>
    normalizedEventType === 'channel_message' || normalizedEventType.startsWith('team_');

  const extractWatchUserContent = (normalizedEventType, payload, data) => {
    const candidates = [
      data?.question,
      payload?.question,
      data?.user_message,
      payload?.user_message,
      data?.user_content,
      payload?.user_content,
      data?.input,
      payload?.input,
      data?.prompt,
      payload?.prompt
    ];
    if (normalizedEventType === 'round_start' || normalizedEventType === 'received') {
      candidates.push(data?.message, payload?.message);
    }
    for (const item of candidates) {
      if (typeof item === 'string' && item.trim()) {
        return item.trim();
      }
    }
    return '';
  };

  const resolveHiddenInternalUserEvent = (payload, data) =>
    Boolean(
      data?.hidden_internal_user ??
        data?.hiddenInternalUser ??
        payload?.hidden_internal_user ??
        payload?.hiddenInternalUser
    );

  const markWatchdogEvent = () => {
    runtime.watchLastEventAt = Date.now();
  };

  // Reconcile from server when watch events appear out-of-sync with stream state.
  const scheduleWatchReconcile = (delayMs = WATCH_RECONCILE_DELAY_MS) => {
    if (controller.signal.aborted) return;
    if (store.activeSessionId !== key) return;
    if (!hasKnownSessionInStore(store, key)) return;
    if (runtime.sendController || runtime.resumeController) return;
    const now = Date.now();
    const nextAllowedAt = Number(runtime.watchReconcileAt) || 0;
    if (nextAllowedAt > now) {
      return;
    }
    runtime.watchReconcileAt = now + WATCH_RECONCILE_COOLDOWN_MS;
    if (runtime.watchReconcileTimer) {
      return;
    }
    runtime.watchReconcileTimer = setTimeout(() => {
      runtime.watchReconcileTimer = null;
      if (controller.signal.aborted) return;
      if (runtime.watchController !== controller) return;
      if (store.activeSessionId !== key) return;
      if (!hasKnownSessionInStore(store, key)) return;
      if (runtime.sendController || runtime.resumeController) return;
      void store.loadSessionDetail(key).catch(() => {});
    }, Math.max(0, Number(delayMs) || 0));
  };

  const startWatchdog = () => {
    if (runtime.watchdogTimer) return;
    const scheduleNext = (delayMs) => {
      if (controller.signal.aborted) return;
      runtime.watchdogTimer = setTimeout(() => {
        runtime.watchdogTimer = null;
        void runWatchdogTick();
      }, Math.max(0, Number(delayMs) || 0));
    };
    const runWatchdogTick = async () => {
      if (controller.signal.aborted) return;
      const profile = resolveWatchdogProfile(store, key);
      if (runtime.sendController || runtime.resumeController || runtime.watchdogBusy) {
        scheduleNext(profile.intervalMs);
        return;
      }
      const lastEventAt = Number(runtime.watchLastEventAt) || 0;
      if (!lastEventAt || Date.now() - lastEventAt < profile.idleMs) {
        scheduleNext(profile.intervalMs);
        return;
      }
      runtime.watchdogBusy = true;
      try {
        if (!hasKnownSessionInStore(store, key)) {
          purgeUnavailableSession(store, key);
          return;
        }
        let response = null;
        try {
          response = await getSessionEvents(key);
        } catch (error) {
          if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
            purgeUnavailableSession(store, key);
            return;
          }
        }
        const payload = response?.data?.data;
        hydrateSessionCommandSessions(key, payload?.command_sessions ?? payload?.commandSessions);
        applySessionRuntimeSnapshot(runtime, payload?.runtime);
        const running = payload?.running;
        const remoteLastEventId = Number(payload?.last_event_id ?? payload?.lastEventId);
        const pendingMessage = findPendingAssistantMessage(sessionMessagesRef);
        if (
          pendingMessage &&
          Number.isFinite(remoteLastEventId) &&
          remoteLastEventId > lastEventId
        ) {
          if (perfEnabled) {
            chatPerf.count('chat_watchdog_resume', 1, { sessionId: key });
          }
          store.resumeStream(key, pendingMessage, {
            force: true,
            afterEventId: lastEventId
          });
          return;
        }
        if (
          running === true &&
          !pendingMessage &&
          Number.isFinite(remoteLastEventId) &&
          remoteLastEventId > lastEventId
        ) {
          scheduleWatchReconcile();
        }
        if (running === false) {
          clearSupersededPendingAssistantMessages(sessionMessagesRef);
          stopPendingAssistantMessage(pendingMessage);
          setSessionLoading(store, key, false);
          notifySessionSnapshot(store, key, sessionMessagesRef, true);
          if (perfEnabled) {
            chatPerf.count('chat_watchdog_idle_complete', 1, { sessionId: key });
          }
        } else if (perfEnabled) {
          chatPerf.count('chat_watchdog_idle', 1, { sessionId: key });
        }
      } finally {
        runtime.watchdogBusy = false;
        if (!controller.signal.aborted && runtime.watchController === controller) {
          const nextProfile = resolveWatchdogProfile(store, key);
          scheduleNext(nextProfile.intervalMs);
        }
      }
    };
    const initialProfile = resolveWatchdogProfile(store, key);
    scheduleNext(initialProfile.intervalMs);
  };

  const onEvent = (eventType, dataText, eventId) => {
    if (runtime.sendController?.signal?.aborted) {
      runtime.sendController = null;
    }
    if (runtime.resumeController?.signal?.aborted) {
      runtime.resumeController = null;
    }
    markWatchdogEvent();
    const payload = safeJsonParse(dataText);
    const data = payload?.data ?? payload;
    const normalizedEventType = String(eventType || '').trim().toLowerCase();
    if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
      const previousThreadStatus = normalizeThreadRuntimeStatus(runtime.threadStatus);
      const normalizedEventId = normalizeStreamEventId(eventId);
      if (normalizedEventId !== null) {
        updateRuntimeLastEventId(runtime, normalizedEventId);
        lastEventId = Math.max(lastEventId, normalizedEventId);
      }
      applySessionRuntimeEvent(store, key, data ?? payload, normalizedEventType);
      const nextThreadStatus = normalizeThreadRuntimeStatus(runtime.threadStatus);
      const runtimeBecameBusy = didThreadRuntimeEnterBusyState(
        previousThreadStatus,
        nextThreadStatus
      );
      if (normalizedEventType === 'thread_status' && runtimeBecameBusy) {
        scheduleWatchReconcile(0);
      }
      return;
    }
    if (
      (runtime.sendController || runtime.resumeController) &&
      !isWatchSidebandEventType(normalizedEventType)
    ) {
      return;
    }
    if (perfEnabled) {
      chatPerf.count('chat_watch_event', 1, { eventType, sessionId: key });
    }
    if (eventType === 'heartbeat' || eventType === 'ping') {
      return;
    }
    handleApprovalEvent(store, eventType, data, requestId, key);
    if (eventType === 'slow_client' && !data) {
      return;
    }
    const stage = data?.stage ?? payload?.stage;
    const eventTimestampMs = resolveTimestampMs(payload?.timestamp ?? data?.timestamp);
    const normalizedEventId = normalizeStreamEventId(eventId);
    if (normalizedEventType === 'channel_message') {
      const result = consumeChatWatchChannelMessage({
        messages: sessionMessagesRef,
        lastEventId,
        eventId,
        eventTimestampMs,
        payload,
        data,
        normalizeEventId: normalizeStreamEventId,
        buildMessage,
        assignStreamEventId,
        insertWatchUserMessage: (content, timestampMs, anchor, options) =>
          insertWatchUserMessage(store, key, sessionMessagesRef, content, timestampMs, anchor, options),
        clearSupersededPendingAssistantMessages,
        dismissStaleInquiryPanels,
        touchUpdatedAt: (timestamp) => touchSessionUpdatedAt(store, key, timestamp),
        notifySnapshot: (immediate = true) =>
          notifySessionSnapshot(store, key, sessionMessagesRef, immediate),
        hiddenInternalUser: resolveHiddenInternalUserEvent(payload, data),
        dedupeAssistantWindowMs: WATCH_USER_MESSAGE_DEDUP_MS
      });
      if (result.handled) {
        if (result.lastEventId > lastEventId) {
          updateRuntimeLastEventId(runtime, result.lastEventId);
          lastEventId = result.lastEventId;
        }
        return;
      }
    }
    const userRoundNumber = normalizeStreamRound(data?.user_round ?? payload?.user_round);
    const directRoundNumber = resolveEventRoundNumber(payload, data);
    const isRoundStart =
      normalizedEventType === 'round_start' ||
      normalizedEventType === 'received' ||
      (normalizedEventType === 'progress' && stage === 'start');
    if (normalizedEventId !== null) {
      if (normalizedEventId <= lastEventId) {
        const latestAssistantTimestamp = resolveLastAssistantTimestampMs(sessionMessagesRef);
        const hasPendingAssistant = Boolean(findPendingAssistantMessage(sessionMessagesRef));
        const highestActiveRound = Array.from(roundStates.keys()).reduce(
          (maxValue, value) => (value > maxValue ? value : maxValue),
          0
        );
        const knownRoundCeiling = Math.max(maxKnownRound, highestActiveRound);
        const directRoundAdvanced =
          directRoundNumber !== null && directRoundNumber > knownRoundCeiling;
        const userRoundAdvanced =
          userRoundNumber !== null && userRoundNumber > knownRoundCeiling;
        const timestampLooksNew =
          !Number.isFinite(eventTimestampMs) ||
          !Number.isFinite(latestAssistantTimestamp) ||
          eventTimestampMs > Number(latestAssistantTimestamp) + 200;
        const canResetByStart =
          isRoundStart &&
          roundStates.size === 0 &&
          !hasPendingAssistant &&
          timestampLooksNew;
        const canResetByRoundHint =
          roundStates.size === 0 &&
          !hasPendingAssistant &&
          timestampLooksNew &&
          isWatchWorkflowEventType(normalizedEventType) &&
          (directRoundAdvanced || userRoundAdvanced);
        if (!canResetByStart && !canResetByRoundHint) {
          if (isWatchWorkflowEventType(normalizedEventType)) {
            scheduleWatchReconcile();
          }
          return;
        }
        setRuntimeLastEventId(runtime, normalizedEventId);
      } else {
        updateRuntimeLastEventId(runtime, normalizedEventId);
      }
      lastEventId = normalizedEventId;
    }
    if (
      normalizedEventId === null &&
      Number.isFinite(minEventTimestampMs) &&
      Number.isFinite(eventTimestampMs) &&
      eventTimestampMs <= minEventTimestampMs
    ) {
      return;
    }
    const roundNumber = resolveWatchRoundNumber(eventType, payload, data, isRoundStart);
    const userContent = extractWatchUserContent(normalizedEventType, payload, data);
    const hiddenInternalUser = resolveHiddenInternalUserEvent(payload, data);
    const shouldPreinsertUserMessage =
      (isRoundStart || normalizedEventType === 'received') && Boolean(userContent);
    if (shouldPreinsertUserMessage) {
      // Insert the new user turn before allocating the assistant round so stale pending
      // assistant content from the previous turn cannot be reused as the current response shell.
      insertWatchUserMessage(store, key, sessionMessagesRef, userContent, eventTimestampMs, null, {
        hiddenInternal: hiddenInternalUser
      });
    }
    const state = ensureRoundState(roundNumber, eventTimestampMs, userRoundNumber, {
      preferFreshRound: isRoundStart || Boolean(userContent)
    });
    if (state && shouldPreinsertUserMessage) {
      state.userInserted = true;
    } else if (state && (isRoundStart || normalizedEventType === 'received')) {
      if (!state.userInserted && userContent) {
        insertWatchUserMessage(
          store,
          key,
          sessionMessagesRef,
          userContent,
          eventTimestampMs,
          state.message,
          { hiddenInternal: hiddenInternalUser }
        );
        state.userInserted = true;
      }
    }
    if (!state) {
      if (isWatchWorkflowEventType(normalizedEventType)) {
        scheduleWatchReconcile();
      }
      return;
    }
    state.message.workflowStreaming = true;
    state.message.stream_incomplete = true;
    assignStreamEventId(state.message, eventId);
    if (perfEnabled) {
      const start = performance.now();
      state.processor.handleEvent(eventType, dataText);
      chatPerf.recordDuration('chat_watch_event_handle', performance.now() - start, {
        eventType,
        sessionId: key
      });
    } else {
      state.processor.handleEvent(eventType, dataText);
    }
    if (eventType === 'final' || eventType === 'error' || eventType === 'queue_fail') {
      finalizeRound(roundNumber, false);
      setSessionLoading(store, key, false);
      if (perfEnabled) {
        chatPerf.count('chat_watch_terminal', 1, { eventType, sessionId: key });
      }
    }
  };

  const baseEventId = lastEventId || 0;
  const watchWithSse = async () => {
    // SSE watch may complete on idle; reconnect to keep behavior close to WS watch mode.
    while (!controller.signal.aborted) {
      const response = await resumeMessageStream(key, {
        signal: controller.signal,
        afterEventId: lastEventId || baseEventId
      });
      if (!response.ok) {
        const errorText = await readResponseError(response);
        throw new Error(errorText || t('chat.error.resumeFailedWithStatus', { status: response.status }));
      }
      await consumeSseStream(response, onEvent);
      if (controller.signal.aborted) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 120));
    }
  };
  const watchWithWs = () =>
    chatWsClient.request({
      requestId,
      sessionId: key,
      message: {
        type: 'watch',
        request_id: requestId,
        session_id: key,
        payload: { after_event_id: baseEventId }
      },
      onEvent,
      signal: controller.signal,
      closeOnFinal: false
    });
  const preferredTransport = resolveStreamTransport();
  store.streamTransport = preferredTransport;
  startWatchdog();
  const watchPromise =
    preferredTransport === 'ws'
      ? watchWithWs().catch((error) => {
          if (error?.phase === 'connect') {
            markWsUnavailable();
            store.streamTransport = 'sse';
            return watchWithSse();
          }
          throw error;
        })
      : watchWithSse();
  watchPromise
    .catch((error) => {
      store.clearPendingApprovals({ requestId, sessionId: key });
      if (error?.name === 'AbortError' || error?.phase === 'aborted') {
        finalizeAll(true);
        return;
      }
      if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
        purgeUnavailableSession(store, key);
        finalizeAll(false);
        return;
      }
      const resumeRequired = error?.phase === 'slow_client' || error?.resumeRequired === true;
      const transient =
        resumeRequired || error?.phase === 'connect' || error?.phase === 'stream' || error?.name === 'TypeError';
      if (transient) {
        if (perfEnabled) {
          chatPerf.count('chat_watch_interrupted', 1, { sessionId: key });
        }
        return;
      }
      finalizeAll(false);
    })
    .finally(() => {
      const runtimeSnapshot = getRuntime(key);
      if (runtimeSnapshot && runtimeSnapshot.watchController === controller) {
        runtimeSnapshot.watchController = null;
        runtimeSnapshot.watchRequestId = null;
        clearWatchdog(runtimeSnapshot);
        if (sessionWatchSessionId === key) {
          sessionWatchSessionId = '';
        }
      }
      if (controller.signal.aborted) {
        return;
      }
      const pendingMessage = findPendingAssistantMessage(sessionMessagesRef);
      if (store.activeSessionId === key && (pendingMessage || !controller.signal.aborted)) {
        setTimeout(() => startSessionWatcher(store, key), 80);
      }
    });
};

const DEFAULT_STREAM_TRANSPORT = 'ws';
const chatWsClient = createWsMultiplexer(() => openChatSocket(), {
  idleTimeoutMs: 30000,
  connectTimeoutMs: 10000,
  pingIntervalMs: 20000
});
let wsUnavailableUntil = 0;
let wsRequestSeq = 0;

const buildWsRequestId = () => {
  wsRequestSeq = (wsRequestSeq + 1) % 1000000;
  return `req_${Date.now().toString(36)}_${wsRequestSeq}`;
};

const markWsUnavailable = (ttlMs = 15000) => {
  const now = Date.now();
  wsUnavailableUntil = now + Math.max(ttlMs, 5000);
};

const resolveStreamTransport = () => {
  if (typeof WebSocket === 'undefined') {
    return 'sse';
  }
  if (wsUnavailableUntil && Date.now() < wsUnavailableUntil) {
    return 'sse';
  }
  const stored = localStorage.getItem('chat_stream_transport');
  if (stored === 'sse' || stored === 'ws') {
    return stored;
  }
  return DEFAULT_STREAM_TRANSPORT;
};

const abortResumeStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  clearSlowClientResume(runtime);
  if (runtime.resumeController) {
    runtime.resumeController.abort();
    runtime.resumeController = null;
  }
  runtime.resumeRequestId = null;
};

const abortSendStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  clearSlowClientResume(runtime);
  if (runtime.sendController) {
    runtime.sendController.abort();
    runtime.sendController = null;
  }
  runtime.sendRequestId = null;
};

const abortCompactRequest = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  if (runtime.compactController) {
    runtime.compactController.abort();
    runtime.compactController = null;
  }
};

const isAbortRequestError = (error: unknown): boolean => {
  const name = String((error as { name?: unknown })?.name || '').trim().toLowerCase();
  const code = String((error as { code?: unknown })?.code || '').trim().toLowerCase();
  const message = String((error as { message?: unknown })?.message || '').trim().toLowerCase();
  if (name === 'aborterror' || name === 'cancelerror' || name === 'cancelederror') {
    return true;
  }
  if (code === 'err_canceled' || code === 'abort_err') {
    return true;
  }
  if (!message) return false;
  return message === 'canceled' || message === 'cancelled' || message.includes('abort');
};

const resolveCompactionWorkflowRefFromMessage = (message): string => {
  const items = Array.isArray(message?.workflowItems) ? message.workflowItems : [];
  for (let cursor = items.length - 1; cursor >= 0; cursor -= 1) {
    const item = items[cursor];
    const ref = String(item?.toolCallId || item?.tool_call_id || '').trim();
    if (ref) return ref;
  }
  return `compaction:manual:${Date.now()}`;
};

const finalizeManualCompactionAsCancelled = (message): void => {
  if (!message || message.role !== 'assistant') return;
  const cancelledDetail = buildDetail({
    stage: 'compacting',
    status: 'cancelled',
    trigger_mode: 'manual',
    error_code: 'MANUAL_COMPACTION_CANCELLED',
    error_message: t('chat.workflow.abortedDetail')
  });
  if (!Array.isArray(message.workflowItems)) {
    message.workflowItems = [];
  }
  if (message.workflowItems.length > 0) {
    message.workflowItems[0].status = 'completed';
    message.workflowItems[0].detail = cancelledDetail;
  }
  const hasCompactionTerminal = message.workflowItems.some(
    (item) => String(item?.eventType || '').trim().toLowerCase() === 'compaction'
  );
  if (!hasCompactionTerminal) {
    message.workflowItems.push(
      buildWorkflowItem(
        t('chat.toolWorkflow.compaction.title'),
        cancelledDetail,
        'completed',
        {
          isTool: true,
          eventType: 'compaction',
          toolName: '上下文压缩',
          toolCallId: resolveCompactionWorkflowRefFromMessage(message)
        }
      )
    );
  }
  message.workflowStreaming = false;
  message.reasoningStreaming = false;
  message.stream_incomplete = false;
  message.resume_available = false;
  message.content = '';
};

const resetChatRuntimeState = () => {
  Array.from(sessionRuntime.keys()).forEach((sessionId) => {
    abortResumeStream(sessionId);
    abortSendStream(sessionId);
    abortCompactRequest(sessionId);
    abortWatchStream(sessionId);
  });
  useCommandSessionStore().reset();
  clearSessionWatcher();
  sessionRuntime.clear();
  sessionMessages.clear();
  sessionListCache.clear();
  sessionListCacheInFlight.clear();
  sessionDetailPrefetchInFlight.clear();
  sessionSubagentsInFlight.clear();
  sessionDetailWarmState.clear();
  sessionHistoryState.clear();
  sessionWorkflowState.clear();
  if (snapshotTimer) {
    clearTimeout(snapshotTimer);
    snapshotTimer = null;
  }
  clearAllChatSnapshots();
};

const scheduleSlowClientResume = (store, sessionId, message, afterEventId) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !message) return;
  const runtime = ensureRuntime(key);
  if (!runtime || runtime.stopRequested) return;
  const normalizedAfterEventId = Math.max(
    normalizeStreamEventId(afterEventId) || 0,
    normalizeStreamEventId(message.stream_event_id) || 0,
    getRuntimeLastEventId(runtime)
  );
  if (normalizedAfterEventId <= 0) return;
  clearSlowClientResume(runtime);
  runtime.slowClientResumeAfterEventId = normalizedAfterEventId;
  runtime.slowClientResumeTimer = setTimeout(() => {
    runtime.slowClientResumeTimer = null;
    const resumeAfterEventId = Math.max(
      normalizeStreamEventId(runtime.slowClientResumeAfterEventId) || 0,
      normalizedAfterEventId
    );
    runtime.slowClientResumeAfterEventId = 0;
    if (runtime.stopRequested || runtime.sendController || runtime.resumeController) {
      return;
    }
    const currentMessages = getSessionMessages(key) || store.messages;
    const targetMessage =
      Array.isArray(currentMessages) && currentMessages.includes(message)
        ? message
        : findPendingAssistantMessage(currentMessages);
    if (!targetMessage || !normalizeFlag(targetMessage.stream_incomplete)) {
      return;
    }
    if (chatPerf.enabled()) {
      chatPerf.count('chat_slow_client_auto_resume', 1, { sessionId: key });
    }
    store.resumeStream(key, targetMessage, { force: true, afterEventId: resumeAfterEventId });
  }, SLOW_CLIENT_RESUME_DELAY_MS);
};

const createWorkflowProcessor = (assistantMessage, workflowState, onSnapshot, options: WorkflowProcessorOptions = {}) => {
  const roundState = normalizeSessionWorkflowState(workflowState);
  const streamFlushMs = resolveStreamFlushMs(null, options.streamFlushMs);
  const perfEnabled = chatPerf.enabled();
  const commandSessionStore = useCommandSessionStore();
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
  }
  let contextEstimateBaseTokens = normalizeContextTokens(stats?.contextTokens);
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

  const MIN_ROUND_SPEED_DECODE_S = 0.2;
  const MAX_ROUND_SPEED_TPS = 10000;

  const recomputeRoundAggregates = () => {
    if (!stats) return;
    let prefillTotal = 0;
    let decodeTotal = 0;
    let hasPrefill = false;
    let hasDecode = false;
    let speedOutputTotal = 0;
    let speedDecodeTotal = 0;
    let speedCount = 0;
    // Keep model-round speed separate from whole-turn usage totals.
    roundMetricsMap.forEach((item) => {
      if (item.prefill !== null) {
        prefillTotal += item.prefill;
        hasPrefill = true;
      }
      if (item.decode !== null) {
        decodeTotal += item.decode;
        hasDecode = true;
      }
      const decode = item.decode !== null && item.decode > 0 ? item.decode : null;
      // Speed definition: model output phase only (from first generated token to end),
      // so prefill/tool time is excluded and decode duration is the only denominator.
      if (item.usage && item.usage.output > 0 && decode !== null && decode > 0) {
        if (decode < MIN_ROUND_SPEED_DECODE_S) {
          return;
        }
        const roundSpeed = item.usage.output / decode;
        if (!Number.isFinite(roundSpeed) || roundSpeed <= 0 || roundSpeed > MAX_ROUND_SPEED_TPS) {
          return;
        }
        speedOutputTotal += item.usage.output;
        speedDecodeTotal += decode;
        speedCount += 1;
      }
    });
    stats.prefill_duration_total_s = hasPrefill ? prefillTotal : null;
    stats.decode_duration_total_s = hasDecode ? decodeTotal : null;
    stats.avg_model_round_speed_tps =
      speedCount > 0 && speedDecodeTotal > 0
        ? speedOutputTotal / speedDecodeTotal
        : null;
    stats.avg_model_round_speed_rounds = speedCount;
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
    const shouldUpdateContextFromUsage = usageOptions.updateContextFromUsage !== false;
    const usageContextTokens = resolveUsageContextTokens(normalizedUsage);
    const existingContextTokens = normalizeContextTokens(stats.contextTokens);
    if (shouldUpdateUsage) {
      stats.usage = normalizedUsage;
    }
    if (shouldUpdateContextFromUsage && usageContextTokens !== null) {
      const changed = existingContextTokens !== usageContextTokens;
      stats.contextTokens = usageContextTokens;
      contextEstimateBaseTokens = usageContextTokens;
      if (changed) {
        options.onContextUsage?.(usageContextTokens, stats.contextTotalTokens ?? null);
      }
    } else if (existingContextTokens !== null && existingContextTokens > 0) {
      contextEstimateBaseTokens = existingContextTokens;
    }
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

  const markQuotaRoundConsumed = (roundNumber) => {
    if (!Number.isFinite(roundNumber)) return false;
    if (consumedQuotaRoundSet.has(roundNumber)) return false;
    consumedQuotaRoundSet.add(roundNumber);
    return true;
  };

  const updateQuotaUsage = (payload, roundNumber = null) => {
    if (!stats) return;
    if (Number.isFinite(roundNumber) && !markQuotaRoundConsumed(roundNumber)) {
      return;
    }
    const rawIncrement =
      payload && typeof payload === 'object' ? payload.consumed ?? payload.count ?? payload.used : null;
    const increment = normalizeStatsCount(rawIncrement);
    const delta = increment > 0 ? increment : 1;
    stats.quotaConsumed = normalizeStatsCount(stats.quotaConsumed) + delta;
    const snapshot = normalizeQuotaSnapshot(payload);
    if (snapshot) {
      stats.quotaSnapshot = snapshot;
    }
  };

  const fallbackQuotaUsageFromRound = (roundNumber) => {
    if (!stats) return;
    if (Number.isFinite(roundNumber)) {
      if (!markQuotaRoundConsumed(roundNumber)) {
        return;
      }
      stats.quotaConsumed = normalizeStatsCount(stats.quotaConsumed) + 1;
      return;
    }
    if (normalizeStatsCount(stats.quotaConsumed) <= 0) {
      stats.quotaConsumed = 1;
    }
  };

  const updateContextUsage = (payload) => {
    if (!stats) return;
    const contextTokens = normalizeContextTokens(
      payload?.context_tokens ??
        payload?.contextTokens ??
        payload?.context ??
        payload?.contextUsage ??
        payload?.context_usage?.context_tokens ??
        payload?.context_usage?.contextTokens
    );
    const contextTotalTokens = normalizeContextTotalTokens(
      payload?.max_context ??
        payload?.maxContext ??
        payload?.context_window ??
        payload?.context_max_tokens ??
        payload?.contextTotalTokens ??
        payload?.context_usage?.max_context ??
        payload?.context_usage?.context_max_tokens
    );
    if (contextTokens !== null) {
      stats.contextTokens = contextTokens;
      contextEstimateBaseTokens = contextTokens;
      if (contextTotalTokens !== null) {
        stats.contextTotalTokens = contextTotalTokens;
      }
      options.onContextUsage?.(contextTokens, contextTotalTokens);
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
    const toolCategory = resolveToolCategory(executeCommandToolName, source);
    const commandIndex = toOptionalInt(source?.command_index, source?.commandIndex);
    const title = buildCommandSessionTitle(command || source?.command, commandIndex);
    const detail = buildDetail({
      ...(source && typeof source === 'object' ? source : {}),
      tool: executeCommandToolName,
      command: pickString(command, source?.command)
    });
    const patch = {
      title,
      detail,
      status: resolveCommandSessionStatus(source, 'loading'),
      isTool: true,
      toolCategory,
      eventType: 'tool_call',
      toolName: executeCommandToolName,
      toolCallId: normalizedSessionId,
      commandSessionId: normalizedSessionId
    };
    const existing = toolCallItemMap.get(normalizedSessionId);
    if (existing) {
      updateWorkflowItem(assistantMessage.workflowItems, existing, patch);
      return existing;
    }
    const item = buildWorkflowItem(title, detail, patch.status, {
      isTool: true,
      toolCategory,
      eventType: 'tool_call',
      toolName: executeCommandToolName,
      toolCallId: normalizedSessionId,
      commandSessionId: normalizedSessionId
    });
    assistantMessage.workflowItems.push(item);
    registerToolItem(executeCommandToolName, item.id, normalizedSessionId);
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
      toolCallId: normalizedSessionId,
      commandSessionId: normalizedSessionId
    };
    const existing = commandSessionResultItemMap.get(normalizedSessionId);
    if (existing) {
      updateWorkflowItem(assistantMessage.workflowItems, existing, patch);
      return existing;
    }
    const item = buildWorkflowItem(title, detail, status, {
      isTool: true,
      toolCategory,
      eventType: 'tool_result',
      toolName: executeCommandToolName,
      toolCallId: normalizedSessionId,
      commandSessionId: normalizedSessionId
    });
    assistantMessage.workflowItems.push(item);
    commandSessionResultItemMap.set(normalizedSessionId, item.id);
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
    clearCompactionProgressRef(workflowRef);
    return true;
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
        eventType: 'compaction'
      });
      finalized = true;
    });
    compactionProgressItemMap.clear();
    activeCompactionWorkflowRef = null;
    return finalized;
  };

  const isCompactionWorkflowEventItem = (item) => {
    const eventType = String(item?.eventType || item?.event || '').trim().toLowerCase();
    return eventType === 'compaction' || eventType === 'compaction_progress';
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

  const resolveCompactionWorkflowRef = (round) => {
    if (Number.isFinite(round)) {
      return `compaction:${round}`;
    }
    if (!activeCompactionWorkflowRef) {
      compactionAnonymousRefSeq += 1;
      activeCompactionWorkflowRef = `compaction:auto:${compactionAnonymousRefSeq}`;
    }
    return activeCompactionWorkflowRef;
  };

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
        payload?.round ??
        data?.user_round ??
        payload?.user_round
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
        const resolved = normalizeStreamRound(
          segment.model_round ?? segment.round ?? segment.user_round
        );
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
  const flushInterval = Math.max(0, Number(streamFlushMs) || 0);
  const scheduleFrame = (callback) =>
    setTimeout(callback, flushInterval || STREAM_FLUSH_BASE_MS);
  const cancelFrame = (timer) => clearTimeout(timer);

  const flushStream = (force = false) => {
    if (streamTimer !== null) {
      cancelFrame(streamTimer);
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
        normalizeContextTokens(stats.contextTokens);
      if (baseTokens !== null && baseTokens > 0) {
        const estimatedOutputTokens = estimateStreamOutputTokens(outputContent);
        if (estimatedOutputTokens > 0) {
          const estimatedTotal = baseTokens + estimatedOutputTokens;
          const currentTokens = normalizeContextTokens(stats.contextTokens);
          if (currentTokens === null || estimatedTotal > currentTokens) {
            stats.contextTokens = estimatedTotal;
            options.onContextUsage?.(estimatedTotal, stats.contextTotalTokens ?? null);
          }
        }
      }
    }
    syncReasoningToMessage();
    if (hasContentDelta || hasReasoningDelta) {
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
    if (streamTimer !== null) return;
    streamTimer = scheduleFrame(() => {
      streamTimer = null;
      flushStream();
    });
  };

  const resetStreamPending = () => {
    if (streamTimer !== null) {
      cancelFrame(streamTimer);
      streamTimer = null;
    }
    pendingContent = '';
    pendingReasoningExplicit = '';
    pendingReasoningFallback = '';
    thinkStreamParser.reset();
  };

  const notifySnapshot = () => {
    if (typeof onSnapshot === 'function') {
      onSnapshot();
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
      const item = buildWorkflowItem('模型输出', '', 'loading');
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

  const handleEvent = (eventName, raw) => {
    const payload = safeJsonParse(raw);
    const data = payload?.data ?? payload;
    const eventType = resolveEventType(eventName, payload);
    applyInteractionTimestamp(payload?.timestamp ?? data?.timestamp);
    if (eventType === 'heartbeat' || eventType === 'ping') {
      return;
    }

    // 基于事件类型生成工作流条目并更新回复内容
      switch (eventType) {
      case 'queue_enter': {
        assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.queued'),
            buildDetail(data ?? payload),
            'pending',
            { eventType: 'queue_enter' }
          )
        );
        break;
      }
      case 'queue_start': {
        assistantMessage.workflowItems.push(
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
        assistantMessage.workflowItems.push(
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
        assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.queueFail'),
            buildDetail(detailPayload),
            'failed',
            { eventType: 'queue_fail' }
          )
        );
        if (!assistantMessage.content) {
          assistantMessage.content = detailText;
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
          const roundNumber = advanceModelRound();
          summary = `调用模型（第 ${roundNumber} 轮）`;
          lastRound = roundNumber;
          // 保持详情中的轮次与会话累计轮次一致
          if (data && typeof data === 'object') {
            detailSource = { ...data, round: roundNumber };
          } else {
            detailSource = { stage, summary: data?.summary ?? payload?.summary ?? '调用模型', round: roundNumber };
          }
        }
        if (stage === 'tool_failure_guard') {
          appendToolFailureGuardWorkflowItem(data ?? payload);
          break;
        }
        if (
          normalizedStage === 'compacting'
          || normalizedStage === 'context_overflow_recovery'
          || normalizedStage === 'context_guard'
        ) {
          // Ignore delayed compaction progress events after terminal state to avoid re-opening "running" UI.
          if (compactionTerminalStatusHint) {
            break;
          }
          summary = resolveCompactionProgressTitle(stage, summary, t) ?? summary;
          const round = resolveRound(payload, data);
          const workflowRef = resolveCompactionWorkflowRef(round);
          ensureCompactionProgressItem(
            pickText(summary) || t('chat.workflow.progressUpdate'),
            buildDetail(detailSource),
            workflowRef
          );
          break;
        }
        const showStage = stage && !['received', 'llm_call'].includes(stage);
        const title = summary ? pickText(summary) : showStage ? `阶段：${stage}` : '进度更新';
        assistantMessage.workflowItems.push(
          buildWorkflowItem(title, buildDetail(detailSource))
        );
        break;
      }
      case 'llm_request': {
        const hasPayload = data && typeof data === 'object' && 'payload' in data;
        const hasSummary = data && typeof data === 'object' && 'payload_summary' in data;
        const title = hasSummary && !hasPayload ? '模型请求摘要' : '模型请求体';
        assistantMessage.workflowItems.push(buildWorkflowItem(title, buildDetail(data)));
        break;
      }
      case 'knowledge_request': {
        const base = data?.knowledge_base ?? data?.knowledgeBase ?? '';
        const title = base ? `知识库请求体（${base}）` : '知识库请求体';
        assistantMessage.workflowItems.push(buildWorkflowItem(title, buildDetail(data)));
        break;
      }
      case 'command_session_delta': {
        break;
      }
      case 'command_session_start': {
        const commandSessionId = extractCommandSessionRef(payload, data);
        if (!commandSessionId) {
          break;
        }
        const detailSource =
          data && typeof data === 'object'
            ? data
            : payload && typeof payload === 'object'
              ? payload
              : {};
        syncCommandSessionSnapshot(detailSource);
        ensureCommandSessionCallItem(commandSessionId, detailSource, detailSource?.command ?? '');
        break;
      }
      case 'command_session_status':
      case 'command_session_exit':
      case 'command_session_summary': {
        const commandSessionId = extractCommandSessionRef(payload, data);
        if (!commandSessionId) {
          break;
        }
        const detailSource =
          data && typeof data === 'object'
            ? data
            : payload && typeof payload === 'object'
              ? payload
              : {};
        syncCommandSessionSnapshot(detailSource);
        const command = pickString(detailSource?.command);
        ensureCommandSessionCallItem(commandSessionId, detailSource, command);
        const outputKey = resolveToolOutputKey(
          executeCommandToolName,
          commandSessionId,
          commandSessionId
        );
        const outputBuffer = getToolOutputBuffer(outputKey);
        if (command && !outputBuffer.command) {
          outputBuffer.command = command;
        }
        if (eventType === 'command_session_summary') {
          mergeCommandSessionSummaryIntoBuffer(outputBuffer, detailSource);
        }
        const toolCategory = resolveToolCategory(executeCommandToolName, detailSource);
        const outputItemId = ensureToolOutputItem(
          executeCommandToolName,
          outputKey,
          toolCategory,
          commandSessionId,
          buildCommandSessionTitle(
            command,
            toOptionalInt(detailSource?.command_index, detailSource?.commandIndex)
          ),
          { commandSessionId }
        );
        if (outputItemId) {
          clearToolOutputFlush(outputKey);
          updateWorkflowItem(assistantMessage.workflowItems, outputItemId, {
            detail: buildToolOutputDetail(outputBuffer),
            status: resolveCommandSessionStatus(
              detailSource,
              eventType === 'command_session_summary' ? 'completed' : 'loading'
            )
          });
        }
        break;
      }
      case 'tool_call': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name ?? '未知工具';
        const toolCallId = extractToolCallRef(payload, data);
        const detailSource = data && typeof data === 'object' ? data : payload ?? data;
        const toolCategory = resolveToolCategory(toolName, data ?? payload);
        if (isExecuteCommandTool(toolName)) {
          registerToolStats(toolName);
          if (lastRound !== null) {
            blockedRounds.add(lastRound);
          }
          break;
        }
        const item = buildWorkflowItem(`调用工具：${toolName}`, buildDetail(detailSource), 'loading', {
          isTool: true,
          toolCategory,
          eventType: 'tool_call',
          toolName: String(toolName || ''),
          toolCallId: toolCallId || undefined
        });
        assistantMessage.workflowItems.push(item);
        registerToolItem(toolName, item.id, toolCallId);
        registerToolStats(toolName);
        if (lastRound !== null) {
          // 工具调用后不再接收该轮后续增量，但保留当前已展示的内容/思考。
          blockedRounds.add(lastRound);
        }
        break;
      }
      case 'tool_output_delta': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name ?? '';
        const toolCallId = extractToolCallRef(payload, data);
        const commandSessionId = extractCommandSessionRef(payload, data);
        const delta = data?.delta ?? payload?.delta ?? '';
        if (!delta) {
          break;
        }
        if (perfEnabled) {
          chatPerf.count('chat_tool_output_delta', 1);
        }
        const streamName = String(data?.stream ?? payload?.stream ?? 'stdout').toLowerCase();
        const command = typeof data?.command === 'string' ? data.command : payload?.command;
        const resolvedToolName = commandSessionId ? executeCommandToolName : toolName;
        const toolCategory = resolveToolCategory(resolvedToolName, data ?? payload);
        if (commandSessionId) {
          syncCommandSessionDelta(commandSessionId, streamName, delta, {
            command,
            command_index: data?.command_index ?? payload?.command_index,
            commandIndex: data?.commandIndex ?? payload?.commandIndex
          });
          ensureCommandSessionCallItem(commandSessionId, data ?? payload ?? {}, command ?? '');
          const outputKey = resolveToolOutputKey(
            resolvedToolName,
            commandSessionId,
            commandSessionId
          );
          const itemId = ensureToolOutputItem(
            resolvedToolName,
            outputKey,
            toolCategory,
            commandSessionId,
            buildCommandSessionTitle(command, data?.command_index ?? payload?.command_index),
            { commandSessionId }
          );
          if (itemId) {
            updateWorkflowItem(assistantMessage.workflowItems, itemId, {
              status: 'loading'
            });
          }
          break;
        }
        const callId = peekToolItemId(toolName, toolCallId);
        const outputKey = resolveToolOutputKey(
          resolvedToolName,
          toolCallId || callId,
          null
        );
        const buffer = getToolOutputBuffer(outputKey);
        if (command && !buffer.command) {
          buffer.command = String(command);
        }
        if (streamName.includes('err')) {
          appendToolOutput(buffer, 'stderr', delta);
        } else {
          appendToolOutput(buffer, 'stdout', delta);
        }
        const itemId = ensureToolOutputItem(
          resolvedToolName,
          outputKey,
          toolCategory,
          toolCallId,
          null,
          null
        );
        if (itemId) {
          scheduleToolOutputFlush(outputKey, itemId);
        }
        break;
      }
      case 'tool_result': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name;
        const toolCallId = extractToolCallRef(payload, data);
        const result = data?.result ?? payload?.result ?? data?.output ?? payload?.output ?? data ?? payload;
        const failed = isFailedResult(payload);
        const targetId = resolveToolItemId(toolName, toolCallId);
        const toolCategory = resolveToolCategory(toolName, data ?? payload);
        const sandboxed = data?.sandbox === true;
        const outputKey = resolveToolOutputKey(toolName, toolCallId || targetId);
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
        const commandSessionRows = isExecuteCommandTool(toolName)
          ? extractCommandSessionResultRows(detailPayload ?? result)
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
            syncCommandSessionSnapshot({
              ...source,
              command_session_id: commandSessionId,
              status: 'exited',
              exit_code: exitCode
            });
            ensureCommandSessionCallItem(
              commandSessionId,
              { ...source, status: 'exited', exit_code: exitCode },
              source?.command ?? ''
            );
            const outputKey = resolveToolOutputKey(
              executeCommandToolName,
              commandSessionId,
              commandSessionId
            );
            const outputBuffer = getToolOutputBuffer(outputKey);
            if (source?.command && !outputBuffer.command) {
              outputBuffer.command = String(source.command);
            }
            if (source?.stdout && !outputBuffer.stdout) {
              outputBuffer.stdout = String(source.stdout);
            }
            if (source?.stderr && !outputBuffer.stderr) {
              outputBuffer.stderr = String(source.stderr);
            }
            const outputItemId = ensureToolOutputItem(
              executeCommandToolName,
              outputKey,
              toolCategory,
              commandSessionId,
              buildCommandSessionTitle(
                source?.command,
                toOptionalInt(source?.command_index, source?.commandIndex)
              ),
              { commandSessionId }
            );
            if (outputItemId) {
              clearToolOutputFlush(outputKey);
            }
            upsertCommandSessionResultItem(commandSessionId, source, rowFailed);
            finalizeToolOutputItem(outputKey, rowFailed);
          });
          break;
        }
        const detail = buildDetail(detailPayload ?? result);
        if (targetId) {
          updateWorkflowItem(assistantMessage.workflowItems, targetId, {
            status: failed ? 'failed' : 'completed'
          });
        }
        finalizeToolOutputItem(outputKey, failed);
        assistantMessage.workflowItems.push(
          buildWorkflowItem(
            `工具结果：${toolName || '未知工具'}`,
            detail,
            failed ? 'failed' : 'completed',
            {
              isTool: true,
              toolCategory,
              eventType: 'tool_result',
              toolName: String(toolName || ''),
              toolCallId: toolCallId || undefined
            }
          )
        );
        if (!assistantMessage.questionPanel && isQuestionPanelToolName(toolName)) {
          const panelPayload = data?.data ?? data?.result ?? data?.output ?? null;
          applyQuestionPanelPayload(panelPayload);
        }
        break;
      }
      case 'approval_request': {
        const approvalId = String(data?.approval_id ?? payload?.approval_id ?? '').trim();
        const toolName = String(data?.tool ?? payload?.tool ?? '').trim();
        const toolCallId = extractToolCallRef(payload, data);
        const toolItemId = peekToolItemId(toolName, toolCallId);
        const title = toolName ? `等待审批：${toolName}` : '等待审批';
        const item = buildWorkflowItem(title, buildDetail(data ?? payload), 'pending', {
          eventType: 'approval_request',
          approvalId: approvalId || undefined,
          toolCallId: toolCallId || undefined
        });
        assistantMessage.workflowItems.push(item);
        if (approvalId) {
          approvalItemMap.set(approvalId, item.id);
        }
        if (toolItemId) {
          updateWorkflowItem(assistantMessage.workflowItems, toolItemId, {
            status: 'pending'
          });
        }
        break;
      }
      case 'approval_result': {
        const approvalId = String(data?.approval_id ?? payload?.approval_id ?? '').trim();
        const toolName = String(data?.tool ?? payload?.tool ?? '').trim();
        const toolCallId = extractToolCallRef(payload, data);
        const toolItemId = peekToolItemId(toolName, toolCallId);
        const statusRaw = String(data?.status ?? payload?.status ?? '').trim().toLowerCase();
        const itemStatus = statusRaw === 'approved' ? 'completed' : 'failed';
        const targetId = approvalId ? approvalItemMap.get(approvalId) : null;
        if (targetId) {
          updateWorkflowItem(assistantMessage.workflowItems, targetId, {
            status: itemStatus,
            detail: buildDetail(data ?? payload),
            eventType: 'approval_result',
            toolCallId: toolCallId || undefined
          });
          approvalItemMap.delete(approvalId);
          if (toolItemId) {
            updateWorkflowItem(assistantMessage.workflowItems, toolItemId, {
              status: statusRaw === 'approved' ? 'loading' : 'failed'
            });
          }
        } else {
          assistantMessage.workflowItems.push(
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
        assistantMessage.workflowItems.push(
          buildWorkflowItem(title, buildDetail(data ?? payload), 'completed')
        );
        if (typeof options.onThreadControl === 'function') {
          try {
            const maybePromise = options.onThreadControl(data ?? payload);
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
        const normalized = applyPlanUpdate(assistantMessage, data);
        if (normalized) {
          assistantMessage.workflowItems.push(
            buildWorkflowItem(
              '计划更新',
              buildDetail({ explanation: normalized.explanation, plan: normalized.steps })
            )
          );
        }
        break;
      }
      case 'question_panel': {
        const appendWorkflow = !assistantMessage.questionPanel;
        applyQuestionPanelPayload(data, { appendWorkflow });
        break;
      }
      case 'slow_client': {
        const capacity = data?.queue_capacity ?? payload?.queue_capacity ?? '-';
        assistantMessage.slow_client = true;
        assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.slowClient'),
            t('chat.workflow.slowClientDetail', { capacity }),
            'failed'
          )
        );
        break;
      }
      case 'llm_output_delta': {
        const round = resolveRound(payload, data);
        if (round !== null) {
          lastRound = round;
          assistantMessage.stream_round = round;
        }
        if (round !== null && blockedRounds.has(round)) {
          break;
        }
        if (round !== null && visibleRound !== round) {
          if (visibleRound === null && outputContent) {
            visibleRound = round;
          } else {
            clearVisibleOutput();
            visibleRound = round;
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
          pendingReasoningExplicit += reasoningDeltaText;
          outputState.reasoningStreaming = true;
        }
        if (typeof delta === 'string' && delta) {
          const parsedDelta = thinkStreamParser.push(delta);
          if (parsedDelta.reasoning) {
            pendingReasoningFallback += parsedDelta.reasoning;
            outputState.reasoningStreaming = true;
          }
          if (parsedDelta.content) {
            pendingContent += parsedDelta.content;
            outputState.streaming = true;
          }
        }
        if (pendingContent || pendingReasoningExplicit || pendingReasoningFallback) {
          ensureOutputItem();
          scheduleStreamFlush();
        }
        break;
      }
      case 'llm_output': {
        const round = resolveRound(payload, data);
        updateUsageStats(
          data?.usage ?? payload?.usage ?? data,
          data?.prefill_duration_s ?? payload?.prefill_duration_s,
          data?.decode_duration_s ?? payload?.decode_duration_s,
          { round, accumulateDurations: true, includeInRoundAverage: true }
        );
        if (round !== null) {
          lastRound = round;
          assistantMessage.stream_round = round;
        }
        if (round !== null && blockedRounds.has(round)) {
          break;
        }
        if (round !== null && visibleRound !== round) {
          if (visibleRound === null && outputContent) {
            visibleRound = round;
          } else {
            clearVisibleOutput();
            visibleRound = round;
          }
        }
        flushStream(true);
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
        if (
          !resolvedHasContent &&
          !hasReasoning &&
          (outputState.streaming || outputState.reasoningStreaming)
        ) {
          outputState.streaming = false;
          outputState.reasoningStreaming = false;
        } else {
          if (reasoningText) {
            outputReasoningExplicit = reasoningText;
          } else if (inlineReasoning) {
            outputReasoningFallback = inlineReasoning;
          }
          if (resolvedHasContent) {
            outputContent = resolvedContent;
            assistantMessage.content = resolvedContent;
          }
          outputState.streaming = false;
          outputState.reasoningStreaming = false;
        }
        syncReasoningToMessage();
        const outputId = ensureOutputItem();
        updateWorkflowItem(assistantMessage.workflowItems, outputId, {
          status: 'completed',
          detail: buildOutputDetail()
        });
        break;
      }
      case 'token_usage': {
        const round = resolveRound(payload, data);
        updateUsageStats(
          data?.usage ?? payload?.usage ?? data,
          null,
          null,
          { round, updateUsage: true, includeInRoundAverage: true }
        );
        break;
      }
      case 'round_usage': {
        const round = resolveRound(payload, data);
        updateUsageStats(
          data?.usage ?? payload?.usage ?? data ?? payload,
          null,
          null,
          // round_usage is whole-turn aggregate; keep final llm_output usage as primary speed source.
          {
            round,
            updateUsage: !stats?.usage,
            updateContextFromUsage: false,
            includeInRoundAverage: false
          }
        );
        fallbackQuotaUsageFromRound(round);
        break;
      }
      case 'context_usage': {
        updateContextUsage(data ?? payload ?? {});
        break;
      }
      case 'compaction': {
        const round = resolveRound(payload, data);
        const workflowRef = Number.isFinite(round)
          ? resolveCompactionWorkflowRef(round)
          : activeCompactionWorkflowRef;
        const normalizedCompactionStatus = String(data?.status ?? payload?.status ?? '').trim().toLowerCase();
        const compactionStatus =
          normalizedCompactionStatus === 'failed' || normalizedCompactionStatus === 'error'
            ? 'failed'
            : 'completed';
        const finalized = finalizeCompactionProgressItem(
          workflowRef,
          data ?? payload ?? {},
          compactionStatus
        );
        if (!finalized) {
          assistantMessage.workflowItems.push(
            buildWorkflowItem(t('chat.toolWorkflow.compaction.title'), buildDetail(data ?? payload), compactionStatus, {
            isTool: true,
            eventType: 'compaction',
            toolName: '上下文压缩',
            toolCallId: workflowRef || undefined
          })
          );
          clearCompactionProgressRef(workflowRef);
        }
        break;
      }
      case 'quota_usage': {
        const round = resolveRound(payload, data);
        updateQuotaUsage(data ?? payload ?? {}, round);
        break;
      }
      case 'final': {
        flushStream(true);
        compactionTerminalStatusHint = 'completed';
        const finalPayload =
          (data && typeof data === 'object' ? data : null)
          ?? (payload && typeof payload === 'object' ? payload : null)
          ?? {};
        finalizeLingeringCompactionProgressItems(finalPayload, 'completed');
        const answer =
          data?.answer ??
          payload?.answer ??
          data?.content ??
          payload?.content ??
          data?.message ??
          payload?.message ??
          raw;
        if (answer) {
          const answerText = pickText(answer, assistantMessage.content);
          const normalizedAnswer = normalizeAssistantOutput(answerText, '');
          assistantMessage.content = normalizedAnswer.content;
          outputContent = normalizedAnswer.content;
          if (normalizedAnswer.inlineReasoning) {
            outputReasoningFallback = normalizedAnswer.inlineReasoning;
          }
          visibleRound = lastRound ?? visibleRound;
        }
        const stopReasonRaw = data?.stop_reason ?? payload?.stop_reason;
        const stopReason = normalizeStopReason(stopReasonRaw);
        if (stopReason) {
          assistantMessage.stop_reason = stopReason;
        }
        if (isToolFailureGuardStopReason(stopReason) && !toolFailureGuardNotified) {
          const stopMeta =
            (data?.stop_meta && typeof data.stop_meta === 'object' ? data.stop_meta : null) ??
            (payload?.stop_meta && typeof payload.stop_meta === 'object'
              ? payload.stop_meta
              : null);
          appendToolFailureGuardWorkflowItem(stopMeta ?? {}, 0);
        }
        if (lastRound !== null) {
          assistantMessage.stream_round = lastRound;
        }
        outputState.streaming = false;
        outputState.reasoningStreaming = false;
        syncReasoningToMessage();
        const keepMarkerLayout = shouldKeepCompactionMarkerLayout();
        const hasOutputTrace = Boolean(
          String(outputContent || '').trim() || String(resolveReasoningOutput() || '').trim()
        );
        if (!keepMarkerLayout || outputItemId || hasOutputTrace) {
          const outputId = ensureOutputItem();
          updateWorkflowItem(assistantMessage.workflowItems, outputId, {
            status: 'completed',
            detail: buildOutputDetail()
          });
        }
        if (!keepMarkerLayout) {
          assistantMessage.workflowItems.push(
            buildWorkflowItem('最终回复', buildDetail(data || answer))
          );
        }
        break;
      }
      case 'error': {
        compactionTerminalStatusHint = 'failed';
        const detail = data?.message ?? payload?.message ?? raw ?? t('chat.error.generic');
        const errorPayload = data && typeof data === 'object'
          ? data
          : payload && typeof payload === 'object'
            ? payload
            : { message: detail };
        const errorCode = String(data?.code ?? payload?.code ?? '').trim().toUpperCase();
        if (errorCode === 'CONTEXT_WINDOW_EXCEEDED' || isContextOverflowText(detail)) {
          markCompactionProgressFailed({
            ...((errorPayload && typeof errorPayload === 'object') ? errorPayload : {}),
            error_code: errorCode || 'CONTEXT_WINDOW_EXCEEDED',
            error_message: String(detail || '')
          });
        }
        finalizeLingeringCompactionProgressItems(errorPayload, 'failed');
        assistantMessage.workflowItems.push(
          buildWorkflowItem(t('chat.workflow.error'), pickText(detail), 'failed', {
            eventType: 'error'
          })
        );
        if (!assistantMessage.content) {
          assistantMessage.content = pickText(detail, t('chat.error.retry'));
        }
        break;
      }
      case 'subagent_dispatch_start': {
        const source = data ?? payload ?? {};
        const dispatchId = String(source?.dispatch_id ?? source?.dispatchId ?? '').trim();
        const label = String(source?.label ?? '').trim();
        const title = label ? `子智能体调度：${label}` : '子智能体调度';
        const detail = buildDetail(source);
        ensureSubagentDispatchItem(dispatchId, title, detail, 'loading');
        break;
      }
      case 'subagent_dispatch_item_update': {
        const source = data ?? payload ?? {};
        const runId = String(source?.run_id ?? source?.runId ?? '').trim();
        const sessionId = String(source?.session_id ?? source?.sessionId ?? '').trim();
        const label = String(source?.label ?? source?.spawn_label ?? source?.title ?? '').trim();
        const titleBase = label || sessionId || runId || '任务';
        const title = `子智能体：${titleBase}`;
        upsertSubagentRunItem(
          runId || sessionId,
          title,
          buildDetail(source),
          normalizeSubagentWorkflowStatus(source?.status),
          { eventType: 'subagent_dispatch_item_update', source }
        );
        break;
      }
      case 'subagent_dispatch_finish': {
        const source = data ?? payload ?? {};
        const dispatchId = String(source?.dispatch_id ?? source?.dispatchId ?? '').trim();
        const title = '子智能体调度结果';
        const detail = buildDetail(source);
        const status = normalizeSubagentWorkflowStatus(source?.status);
        ensureSubagentDispatchItem(dispatchId, title, detail, status);
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
        const runKey = String(
          sourceObject?.run_id ??
            sourceObject?.runId ??
            sourceObject?.session_id ??
            sourceObject?.sessionId ??
            ''
        ).trim();
        upsertSubagentRunItem(
          runKey,
          titleMap[eventType] || '子智能体事件',
          buildDetail(sourceObject),
          normalizeSubagentWorkflowStatus((data ?? payload ?? {})?.status),
          { eventType, source: sourceObject }
        );
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
        assistantMessage.workflowItems.push(
          buildWorkflowItem(t('chat.workflow.event', { event: eventType }), buildDetail(data || raw), status)
        );
        break;
      }
      default: {
        const fallbackName = data?.name ?? payload?.name;
        const summary = fallbackName
          ? t('chat.workflow.eventWithName', { event: eventType, name: fallbackName })
          : t('chat.workflow.event', { event: eventType });
        assistantMessage.workflowItems.push(buildWorkflowItem(summary, buildDetail(data || raw)));
        break;
      }
    }
    notifySnapshot();
  };

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

const hydrateMessage = (message, workflowState) => {
  if (!message || message.role !== 'assistant') {
    return message;
  }
  const normalizedOutput = normalizeAssistantOutput(
    message.content,
    resolveAssistantReasoning(message)
  );
  const hydrated = {
    ...message,
    content: normalizedOutput.content,
    workflowItems: [],
    workflowStreaming: normalizeFlag(message?.workflowStreaming),
    stream_incomplete: normalizeFlag(message?.stream_incomplete),
    reasoning: normalizedOutput.reasoning,
    reasoningStreaming: normalizeFlag(message?.reasoningStreaming),
    hiddenInternal: normalizeHiddenInternalMessage(message?.hiddenInternal),
    subagents: normalizeMessageSubagents(message?.subagents),
    feedback: normalizeMessageFeedback(message?.feedback),
    stats: normalizeMessageStats(message.stats) || buildMessageStats()
  };
  const plan = normalizePlanPayload(message.plan);
  hydrated.plan = plan;
  hydrated.planVisible = shouldAutoShowPlan(plan, message);
  hydrated.questionPanel = normalizeInquiryPanelState(message.questionPanel);
  if (Array.isArray(message.workflow_events) && message.workflow_events.length > 0) {
    const processor = createWorkflowProcessor(
      hydrated,
      workflowState,
      null,
      {
        finalizeWithNow: false,
        streamFlushMs: STREAM_FLUSH_BASE_MS,
        sessionId: message?.session_id ?? message?.sessionId ?? null
      }
    );
    message.workflow_events.forEach((event) => {
      processor.handleEvent(event?.event || '', event?.raw || '');
    });
    processor.finalize();
  }
  return hydrated;
};

export const useChatStore = defineStore('chat', {
  state: () => ({
    sessions: [],
    activeSessionId: null,
    messages: [],
    loadingBySession: {},
    greetingOverride: '',
    draftAgentId: '',
    draftToolOverrides: null,
    pendingApprovals: [] as PendingApproval[],
    streamTransport: resolveStreamTransport()
  }),
  getters: {
    isSessionLoading: (state) => (sessionId) => {
      const key = resolveSessionKey(sessionId);
      if (!key) return false;
      return Boolean(state.loadingBySession[key]);
    },
    isSessionBusy: (state) => (sessionId) => {
      const key = resolveSessionKey(sessionId);
      if (!key) return false;
      const activeKey = resolveSessionKey(state.activeSessionId);
      const messages = activeKey === key ? state.messages : getSessionMessages(key);
      return isSessionBusyFromSignals(
        state.loadingBySession[key],
        messages,
        getRuntime(key)?.threadStatus
      );
    },
    sessionRuntimeStatus: () => (sessionId) => {
      const runtime = getRuntime(sessionId);
      return normalizeThreadRuntimeStatus(runtime?.threadStatus);
    },
    historyLoading: () => (sessionId) => {
      const state = getHistoryState(sessionId);
      return Boolean(state?.loading);
    },
    canLoadMoreHistory: () => (sessionId) => {
      const state = getHistoryState(sessionId);
      return Boolean(state?.hasMore) && !state?.loading;
    },
    historyBeforeId: () => (sessionId) => {
      const state = getHistoryState(sessionId);
      return state?.beforeId ?? null;
    },
    activeApproval: (state) => (Array.isArray(state.pendingApprovals) ? state.pendingApprovals[0] : null)
  },
  actions: {
    markPageUnloading() {
      pageUnloading = true;
      clearSessionWatcher();
    },
    resetState() {
      pageUnloading = false;
      resetChatRuntimeState();
      this.$reset();
    },
    setStreamTransport(transport) {
      const next = transport === 'sse' ? 'sse' : 'ws';
      localStorage.setItem('chat_stream_transport', next);
      if (next === 'ws') {
        wsUnavailableUntil = 0;
      }
      this.streamTransport = next;
    },
    async refreshStreamTransportPolicy() {
      try {
        const { data } = await fetchChatTransportProfile();
        const remote = String(data?.data?.chat_stream_channel || '').trim().toLowerCase();
        const next = remote === 'sse' ? 'sse' : 'ws';
        localStorage.setItem('chat_stream_transport', next);
        if (next === 'ws') {
          wsUnavailableUntil = 0;
        }
        this.streamTransport = resolveStreamTransport();
        return next;
      } catch (error) {
        this.streamTransport = resolveStreamTransport();
        return this.streamTransport;
      }
    },
    toggleStreamTransport() {
      const next = this.streamTransport === 'sse' ? 'ws' : 'sse';
      this.setStreamTransport(next);
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
    getPersistedState() {
      return readChatPersistState();
    },
    getLastSessionId(agentId) {
      return resolvePersistedSessionId(agentId);
    },
    setLastSessionId(agentId, sessionId) {
      persistAgentSession(agentId, sessionId);
    },
    hasSessionMessages(sessionId) {
      const cached = getSessionMessages(sessionId);
      return Array.isArray(cached) && cached.length > 0;
    },
    getCachedSessionMessages(sessionId) {
      const cached = getSessionMessages(sessionId);
      return Array.isArray(cached) ? cached : [];
    },
    getCachedSessions(agentId) {
      const cached = readSessionListCache(agentId);
      if (cached) return cached;
      return filterSessionsByAgent(agentId, this.sessions);
    },
    resolveReusableFreshSessionId(agentId) {
      const requestedAgentId = String(agentId ?? '').trim();
      const normalizedAgentId =
        requestedAgentId === DEFAULT_AGENT_KEY ? '' : requestedAgentId;
      const activeSessionId = resolveSessionKey(this.activeSessionId);
      const sessions = filterSessionsByAgent(normalizedAgentId, this.sessions);
      for (const session of sessions) {
        const sessionId = resolveSessionKey(session?.id);
        if (!sessionId) continue;
        const fallbackMessages = sessionId === activeSessionId ? this.messages : null;
        if (isReusableFreshSession(session, fallbackMessages)) {
          return sessionId;
        }
      }
      return '';
    },
    resolveInitialSessionId(agentId, sourceSessions = null) {
      const targetSessions = Array.isArray(sourceSessions) ? sourceSessions : this.getCachedSessions(agentId);
      return resolveInitialSessionIdFromList(agentId, targetSessions);
    },
    async prefetchAgentSessions(agentId) {
      const normalizedAgentId = String(agentId ?? '').trim();
      const cached = readSessionListCache(normalizedAgentId);
      if (cached) {
        return cached;
      }
      const cacheKey = resolveSessionListCacheKey(normalizedAgentId);
      const inFlight = sessionListCacheInFlight.get(cacheKey);
      if (inFlight) {
        return inFlight;
      }
      const request = (async () => {
        const params = { agent_id: normalizedAgentId };
        const { data } = await listSessions(params);
        const sessions = sortSessionsByActivity(data?.data?.items || []);
        writeSessionListCache(normalizedAgentId, sessions);
        return cloneSessionList(sessions);
      })().finally(() => {
        sessionListCacheInFlight.delete(cacheKey);
      });
      sessionListCacheInFlight.set(cacheKey, request);
      return request;
    },
    async listSessionsByStatus(options: ListSessionsByStatusOptions = {}) {
      const params: { agent_id?: string; status?: string } = {};
      if (Object.prototype.hasOwnProperty.call(options, 'agent_id')) {
        params.agent_id = String(options.agent_id ?? '');
      }
      const status = String(options.status || '').trim().toLowerCase();
      if (status === 'active' || status === 'archived' || status === 'all') {
        params.status = status;
      }
      const { data } = await listSessions(Object.keys(params).length ? params : undefined);
      return sortSessionsByActivity(data?.data?.items || []);
    },
    async preloadSessionDetail(sessionId) {
      const targetId = resolveSessionKey(sessionId);
      if (!targetId) return null;
      if (!hasKnownSessionInStore(this, targetId)) {
        purgeUnavailableSession(this, targetId);
        return null;
      }
      if (isSessionDetailWarm(targetId) && getSessionMessages(targetId)?.length) {
        return this.sessions.find((session) => session.id === targetId) || null;
      }
      const inFlight = sessionDetailPrefetchInFlight.get(targetId);
      if (inFlight) {
        return inFlight;
      }
      const request = (async () => {
        let sessionRes = null;
        let eventsRes = null;
        try {
          [sessionRes, eventsRes] = await Promise.all([
            getSession(targetId),
            getSessionEvents(targetId).catch((error) => {
              if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
                throw error;
              }
              return null;
            })
          ]);
        } catch (error) {
          if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
            purgeUnavailableSession(this, targetId);
            return null;
          }
          throw error;
        }
        const payload = sessionRes?.data;
        const sessionDetail = payload?.data || null;
        const eventsPayload = eventsRes?.data?.data || null;
        hydrateSessionCommandSessions(
          targetId,
          eventsPayload?.command_sessions ?? eventsPayload?.commandSessions
        );
        applySessionRuntimeSnapshot(ensureRuntime(targetId), eventsPayload?.runtime);
        updateRuntimeLastEventId(
          ensureRuntime(targetId),
          normalizeStreamEventId(eventsPayload?.last_event_id ?? eventsPayload?.lastEventId)
        );
        const rounds = eventsPayload?.rounds || [];
        const workflowState = buildSessionWorkflowState();
        const rawMessages = attachWorkflowEvents(sessionDetail?.messages || [], rounds);
        let messages = rawMessages.map((message) => hydrateMessage(message, workflowState));
        messages = dedupeAssistantMessages(messages);
        dismissStaleInquiryPanels(messages);
        const greetingMessages = ensureGreetingMessage(messages, {
          createdAt: sessionDetail?.created_at,
          greeting: this.greetingOverride
        });
        applyHistoryMeta(targetId, sessionDetail, greetingMessages);
        applyMessageWindow(this, targetId, greetingMessages);
        cacheSessionMessages(targetId, greetingMessages);
        markSessionDetailWarm(targetId);
        void this.refreshSessionSubagents(targetId).catch(() => null);
        return sessionDetail;
      })().finally(() => {
        sessionDetailPrefetchInFlight.delete(targetId);
      });
      sessionDetailPrefetchInFlight.set(targetId, request);
      return request;
    },
    getSnapshotForSession(sessionId) {
      const snapshot = readChatSnapshot();
      if (!snapshot || snapshot.sessionId !== String(sessionId || '')) {
        return null;
      }
      return snapshot;
    },
    scheduleSnapshot(immediate = false) {
      scheduleChatSnapshot(this, immediate);
    },
    setGreetingOverride(content) {
      const next = String(content || '').trim();
      this.greetingOverride = next;
      const greetingIndex = this.messages.findIndex((message) => message?.isGreeting);
      if (greetingIndex < 0) return;
      const greetingText = resolveGreetingContent(next);
      if (this.messages[greetingIndex].content !== greetingText) {
        this.messages[greetingIndex].content = greetingText;
        this.scheduleSnapshot(true);
      }
    },
    resolveInquiryPanel(message, patch: InquiryPanelPatch = {}) {
      if (!message || message.role !== 'assistant') return;
      const panel = normalizeInquiryPanelState(message.questionPanel);
      if (!panel) return;
      message.questionPanel = {
        ...panel,
        status: normalizeInquiryPanelStatus(patch.status ?? panel.status),
        selected: Array.isArray(patch.selected) ? patch.selected : panel.selected
      };
      this.scheduleSnapshot(true);
    },
    async refreshSessionSubagents(sessionId) {
      const targetSessionId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetSessionId) return [];
      const inFlight = sessionSubagentsInFlight.get(targetSessionId);
      if (inFlight) {
        return inFlight;
      }
      const request = getSessionSubagents(targetSessionId)
        .then(({ data }) => {
          const items = Array.isArray(data?.data?.items) ? data.data.items : [];
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
      await this.refreshSessionSubagents(targetSessionId);
      return data?.data || null;
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
        const incoming = Array.isArray(payload.messages) ? payload.messages : [];
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
    appendLocalMessage(role: string, content: string, options: AppendLocalMessageOptions = {}) {
      const normalizedRole = role === 'assistant' ? 'assistant' : 'user';
      const text = String(content || '').trim();
      if (!text) return null;
      const message = buildMessage(normalizedRole, text, options.createdAt);
      this.messages.push(message);
      const targetSessionId = String(options.sessionId ?? this.activeSessionId ?? '').trim();
      if (targetSessionId) {
        cacheSessionMessages(targetSessionId, this.messages);
        touchSessionUpdatedAt(this, targetSessionId, message.created_at || Date.now());
        notifySessionSnapshot(this, targetSessionId, this.messages, options.immediate !== false);
      } else {
        this.scheduleSnapshot(options.immediate !== false);
      }
      return message;
    },
    async loadSessions(options: LoadSessionsOptions = {}) {
      const shouldRefreshTransport =
        options.skipTransportRefresh !== true && options.refresh_transport !== false;
      if (shouldRefreshTransport) {
        await this.refreshStreamTransportPolicy();
      }
      const params: { agent_id?: string } = {};
      let requestedAgentId: string | null = null;

      if (Object.prototype.hasOwnProperty.call(options, 'agent_id')) {
        requestedAgentId = String(options.agent_id ?? '');
        params.agent_id = requestedAgentId;
      }
      const { data } = await listSessions(Object.keys(params).length ? params : undefined);
      this.sessions = sortSessionsByActivity(data.data.items || []);
      if (requestedAgentId !== null) {
        writeSessionListCache(requestedAgentId, this.sessions);
      }
      syncDemoChatCache({ sessions: this.sessions });
      return this.sessions;
    },
    openDraftSession(options: OpenDraftSessionOptions = {}) {
      const currentSessionId = this.activeSessionId;
      cacheSessionMessages(currentSessionId, this.messages);
      abortResumeStream(currentSessionId);
      clearSessionWatcher();
      const runtime = ensureRuntime(currentSessionId);
      if (runtime) {
        runtime.stopRequested = false;
      }
      // Keep in-flight send stream alive so switching agent/thread won't cancel background runs.
      if (!runtime?.sendController) {
        setSessionLoading(this, currentSessionId, false);
      }
      this.activeSessionId = null;
      this.draftAgentId = String(options.agent_id || '').trim();
      this.draftToolOverrides = null;
      this.messages = ensureGreetingMessage([], { greeting: this.greetingOverride });
      persistDraftSession();
    },
    setDraftToolOverrides(overrides) {
      if (!Array.isArray(overrides) || overrides.length === 0) {
        this.draftToolOverrides = null;
        return;
      }
      this.draftToolOverrides = [...overrides];
    },
    async createSession(payload: Record<string, unknown> = {}) {
      abortResumeStream(this.activeSessionId);
      clearSessionWatcher();
      const { data } = await createSession(payload);
      const session = data.data;
      this.sessions.unshift(session);
      if (session?.is_main === true) {
        this.sessions = applyMainSession(this.sessions, session.agent_id, session.id);
      }
      writeSessionListCache(session.agent_id, filterSessionsByAgent(session.agent_id, this.sessions));
      this.activeSessionId = session.id;
      this.draftAgentId = String(session.agent_id || '').trim();
      this.messages = ensureGreetingMessage([], {
        createdAt: session.created_at,
        greeting: this.greetingOverride
      });
      cacheSessionMessages(session.id, this.messages);
      touchSessionUpdatedAt(this, session.id, session.updated_at || session.created_at);
      getSessionWorkflowState(session.id, { reset: true });
      getHistoryState(session.id, { reset: true });
      persistActiveSession(session.id, session.agent_id);
      syncDemoChatCache({
        sessions: this.sessions,
        sessionId: this.activeSessionId,
        messages: this.messages
      });
      if (session?.is_main !== true) {
        try {
          await this.setMainSession(session.id);
        } catch (error) {
          // Keep local session state when explicit main-session sync fails.
        }
      }
      startSessionWatcher(this, session.id);
      return session;
    },
    async setMainSession(sessionId) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return null;
      const targetSession = this.sessions.find((item) => item.id === targetId) || null;
      const agentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      const apiAgentId = agentId || DEFAULT_AGENT_KEY;
      await setDefaultSession(apiAgentId, { session_id: targetId });
      this.sessions = applyMainSession(this.sessions, agentId, targetId);
      writeSessionListCache(agentId, filterSessionsByAgent(agentId, this.sessions));
      persistAgentSession(agentId, targetId);
      return targetId;
    },

    async loadSessionDetail(sessionId) {
      const targetSessionId = resolveSessionKey(sessionId);
      if (!targetSessionId) return null;
      const previousSessionId = this.activeSessionId;
      if (previousSessionId && previousSessionId !== targetSessionId) {
        cacheSessionMessages(previousSessionId, this.messages);
      }
      abortResumeStream(previousSessionId);
      clearSessionWatcher();
      this.activeSessionId = targetSessionId;
      getHistoryState(targetSessionId, { reset: true });
      const cachedSessionMessages = dedupeAssistantMessagesInPlace(getSessionMessages(targetSessionId));
      const snapshot = this.getSnapshotForSession(targetSessionId);
      if (cachedSessionMessages?.length) {
        this.messages = ensureGreetingMessage(cachedSessionMessages, {
          greeting: this.greetingOverride
        });
      } else if (snapshot?.messages?.length) {
        const cachedMessages = dedupeAssistantMessages(
          snapshot.messages
          .map((item) => normalizeSnapshotMessage(item))
          .filter(Boolean)
        );
        this.messages = ensureGreetingMessage(cachedMessages, {
          greeting: this.greetingOverride
        });
      }
      if (cachedSessionMessages?.length || snapshot?.messages?.length) {
        cacheSessionMessages(targetSessionId, this.messages);
      }
      if (!hasKnownSessionInStore(this, targetSessionId)) {
        purgeUnavailableSession(this, targetSessionId);
        return null;
      }
      let sessionRes = null;
      let eventsRes = null;
      try {
        [sessionRes, eventsRes] = await Promise.all([
          getSession(targetSessionId),
          getSessionEvents(targetSessionId).catch((error) => {
            if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
              throw error;
            }
            return null;
          })
        ]);
      } catch (error) {
        if (isSessionUnavailableStatus(resolveChatHttpStatus(error))) {
          purgeUnavailableSession(this, targetSessionId);
          return null;
        }
        throw error;
      }
      const data = sessionRes?.data;
      const sessionDetail = data?.data || null;
      const eventsPayload = eventsRes?.data?.data || null;
      hydrateSessionCommandSessions(
        targetSessionId,
        eventsPayload?.command_sessions ?? eventsPayload?.commandSessions
      );
      applySessionRuntimeSnapshot(ensureRuntime(targetSessionId), eventsPayload?.runtime);
      const remoteRunning = eventsPayload?.running === true;
      const remoteLastEventId = normalizeStreamEventId(
        eventsPayload?.last_event_id ?? eventsPayload?.lastEventId
      );
      updateRuntimeLastEventId(ensureRuntime(targetSessionId), remoteLastEventId);
      const sessionCreatedAt = sessionDetail?.created_at;
      if (sessionDetail?.id) {
        const index = this.sessions.findIndex((item) => item.id === sessionDetail.id);
        if (index >= 0) {
          this.sessions[index] = { ...this.sessions[index], ...sessionDetail };
        } else {
          this.sessions.unshift(sessionDetail);
        }
      }
      const resolvedAgentId =
        sessionDetail?.agent_id ??
        this.sessions.find((item) => item.id === targetSessionId)?.agent_id ??
        '';
      const resolvedAgentIdText = String(resolvedAgentId || '').trim();
      writeSessionListCache(
        resolvedAgentIdText,
        filterSessionsByAgent(resolvedAgentIdText, this.sessions)
      );
      const rounds = eventsPayload?.rounds || [];
      const workflowState = getSessionWorkflowState(targetSessionId, { reset: true });
      const rawMessages = attachWorkflowEvents(sessionDetail?.messages || [], rounds);
      let messages = rawMessages.map((message) =>
        hydrateMessage(message, workflowState)
      );
      messages = mergeSnapshotIntoMessages(messages, snapshot);
      const finalCachedMessages = dedupeAssistantMessages(getSessionMessages(targetSessionId));
      messages = mergeCompactionMarkersIntoMessages(messages, finalCachedMessages);
      messages = dedupeAssistantMessages(messages);
      if (!remoteRunning) {
        clearCompletedAssistantStreamingState(messages);
      }
      if (!remoteRunning) {
        clearCompletedAssistantStreamingState(finalCachedMessages);
      }
      if (remoteRunning && shouldPreferCachedMessages(finalCachedMessages, messages)) {
        messages = dedupeAssistantMessages(
          mergeSnapshotIntoMessages(finalCachedMessages, { messages })
        );
      }
      dismissStaleInquiryPanels(messages);
      const nextMessages = ensureGreetingMessage(messages, {
        createdAt: sessionCreatedAt,
        greeting: this.greetingOverride
      });
      if (!remoteRunning) {
        clearCompletedAssistantStreamingState(nextMessages);
      }
      clearSupersededPendingAssistantMessages(nextMessages);
      applyHistoryMeta(targetSessionId, sessionDetail, nextMessages);
      cacheSessionMessages(targetSessionId, nextMessages);
      markSessionDetailWarm(targetSessionId);
      // Ignore stale async response: keep current foreground conversation state untouched.
      if (resolveSessionKey(this.activeSessionId) !== targetSessionId) {
        return data.data;
      }
      this.draftAgentId = resolvedAgentIdText;
      persistActiveSession(targetSessionId, resolvedAgentIdText);
      this.draftToolOverrides = null;
      this.messages = nextMessages;
      applyMessageWindow(this, targetSessionId, this.messages);
      syncDemoChatCache({ sessionId: targetSessionId, messages: this.messages });
      const pendingMessage = findPendingAssistantMessage(this.messages);
      if (pendingMessage && remoteRunning) {
        const resumeAfterEventId =
          normalizeStreamEventId(pendingMessage.stream_event_id) ?? remoteLastEventId;
        if (
          resumeAfterEventId !== null &&
          resumeAfterEventId > 0 &&
          normalizeStreamEventId(pendingMessage.stream_event_id) === null
        ) {
          pendingMessage.stream_event_id = resumeAfterEventId;
        }
        this.resumeStream(
          targetSessionId,
          pendingMessage,
          resumeAfterEventId !== null && resumeAfterEventId > 0
            ? { afterEventId: resumeAfterEventId }
            : {}
        );
      } else if (!remoteRunning) {
        setSessionLoading(this, targetSessionId, false);
      }
      this.scheduleSnapshot(true);
      startSessionWatcher(this, targetSessionId);
      void this.refreshSessionSubagents(targetSessionId).catch(() => null);
      return data.data;
    },
    async loadOlderHistory(sessionId, options: { limit?: number; beforeId?: number } = {}) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetId) return [];
      const state = getHistoryState(targetId);
      if (state.loading || state.hasMore === false) return [];
      const perfEnabled = chatPerf.enabled();
      const perfStart = perfEnabled ? performance.now() : 0;
      const limit = normalizeHistoryPageLimit(options.limit ?? HISTORY_PAGE_LIMIT);
      const beforeIdRaw = options.beforeId ?? state.beforeId;
      const beforeId = Number.isFinite(Number(beforeIdRaw))
        ? Number.parseInt(String(beforeIdRaw), 10)
        : null;
      state.loading = true;
      try {
        const { data } = await getSessionHistoryPage(targetId, {
          before_id: beforeId,
          limit
        });
        const payload = data?.data || {};
        const incoming = Array.isArray(payload.messages) ? payload.messages : [];
        const incomingHasMore =
          payload.history_has_more ??
          payload.historyHasMore ??
          payload.history_more ??
          payload.historyMore ??
          false;
        const incomingBeforeId =
          payload.history_before_id ??
          payload.historyBeforeId ??
          payload.history_before ??
          payload.historyBefore ??
          null;
        const existingIds = new Set(
          (Array.isArray(this.messages) ? this.messages : [])
            .map((message) => Number.parseInt(String(message?.history_id ?? ''), 10))
            .filter((value) => Number.isFinite(value) && value > 0)
        );
        const deduped = incoming.filter((message) => {
          const id = Number.parseInt(String(message?.history_id ?? ''), 10);
          if (Number.isFinite(id) && id > 0) {
            if (existingIds.has(id)) return false;
            existingIds.add(id);
          }
          return true;
        });
        if (deduped.length > 0) {
          const sessionMessagesRef = resolveSessionKey(this.activeSessionId) === targetId
            ? this.messages
            : getSessionMessages(targetId) || [];
          const nextMessages = [...deduped, ...sessionMessagesRef];
          const nextLimit = Math.min(
            Number(state.windowLimit || MESSAGE_WINDOW_LIMIT) + deduped.length,
            MESSAGE_WINDOW_MAX
          );
          state.windowLimit = nextLimit;
          if (resolveSessionKey(this.activeSessionId) === targetId) {
            this.messages = nextMessages;
            notifySessionSnapshot(this, targetId, this.messages, true, { skipWindowing: true });
          } else {
            cacheSessionMessages(targetId, nextMessages);
          }
        }
        const resolvedBeforeId = Number.parseInt(String(incomingBeforeId ?? ''), 10);
        state.beforeId = Number.isFinite(resolvedBeforeId) && resolvedBeforeId > 0
          ? resolvedBeforeId
          : findOldestHistoryId(this.messages);
        state.hasMore = Boolean(incomingHasMore) && Boolean(state.beforeId);
        if (perfEnabled) {
          chatPerf.recordDuration('chat_history_load', performance.now() - perfStart, {
            sessionId: targetId,
            incoming: incoming.length,
            appended: deduped.length,
            hasMore: state.hasMore
          });
        }
        return deduped;
      } catch (error) {
        if (perfEnabled) {
          chatPerf.count('chat_history_load_failed', 1, { sessionId: targetId });
        }
        return [];
      } finally {
        state.loading = false;
      }
    },
    async renameSession(sessionId, title) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      const nextTitle = String(title || '').trim();
      if (!targetId || !nextTitle) return null;
      const { data } = await renameSessionApi(targetId, { title: nextTitle });
      const updated = data?.data || null;
      const index = this.sessions.findIndex((item) => resolveSessionKey(item?.id) === targetId);
      if (index >= 0) {
        const previous = this.sessions[index] || {};
        this.sessions[index] = {
          ...previous,
          ...(updated && typeof updated === 'object' ? updated : {}),
          id: targetId,
          title: String((updated && updated.title) || nextTitle).trim() || nextTitle
        };
        this.sessions = sortSessionsByActivity(this.sessions);
        const targetAgentId = String(this.sessions[index]?.agent_id || '').trim();
        writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
        syncDemoChatCache({ sessions: this.sessions });
      }
      return updated;
    },
    async archiveSession(sessionId) {
      const targetId = resolveSessionKey(sessionId || this.activeSessionId);
      if (!targetId) return null;
      if (resolveSessionKey(targetId) === resolveSessionKey(this.activeSessionId)) {
        clearSessionWatcher();
      }
      const targetSession = this.sessions.find((item) => resolveSessionKey(item?.id) === targetId) || null;
      const targetAgentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      abortResumeStream(targetId);
      abortSendStream(targetId);
      setSessionLoading(this, targetId, false);
      this.clearPendingApprovals({ sessionId: targetId });
      sessionRuntime.delete(resolveSessionKey(targetId));
      sessionMessages.delete(resolveSessionKey(targetId));
      sessionDetailWarmState.delete(resolveSessionKey(targetId));
      sessionDetailPrefetchInFlight.delete(resolveSessionKey(targetId));
      sessionSubagentsInFlight.delete(resolveSessionKey(targetId));
      sessionHistoryState.delete(resolveSessionKey(targetId));
      const { data } = await archiveSessionApi(targetId);
      const archived = data?.data || null;
      this.sessions = this.sessions.filter((item) => resolveSessionKey(item?.id) !== targetId);
      if (targetSession?.is_main) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        const apiAgentId = targetAgentId || DEFAULT_AGENT_KEY;
        if (fallback) {
          await setDefaultSession(apiAgentId, { session_id: fallback.id });
          this.sessions = applyMainSession(this.sessions, targetAgentId, fallback.id);
          persistAgentSession(targetAgentId, fallback.id);
        } else {
          this.sessions = applyMainSession(this.sessions, targetAgentId, '');
          persistAgentSession(targetAgentId, '');
        }
      }
      sessionWorkflowState.delete(String(targetId));
      removeDemoChatSession(targetId);
      clearChatSnapshot(targetId);
      if (resolvePersistedSessionId(targetAgentId) === targetId) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        persistAgentSession(targetAgentId, fallback?.id || '');
      }
      writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
      if (this.activeSessionId === targetId) {
        const nextSession = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        if (nextSession) {
          await this.loadSessionDetail(nextSession.id);
        } else {
          this.openDraftSession({ agent_id: targetAgentId });
        }
      }
      return archived;
    },
    async restoreSession(sessionId) {
      const targetId = resolveSessionKey(sessionId);
      if (!targetId) return null;
      const { data } = await restoreSessionApi(targetId);
      const restored = data?.data || null;
      if (!restored || typeof restored !== 'object') {
        return restored;
      }
      const resolvedId = resolveSessionKey(restored.id || targetId);
      if (!resolvedId) {
        return restored;
      }
      const index = this.sessions.findIndex((item) => resolveSessionKey(item?.id) === resolvedId);
      if (index >= 0) {
        this.sessions[index] = { ...this.sessions[index], ...restored, id: resolvedId };
      } else {
        this.sessions.unshift({ ...restored, id: resolvedId });
      }
      const restoredAgentId = String(restored.agent_id || '').trim();
      if (restored?.is_main) {
        this.sessions = applyMainSession(this.sessions, restoredAgentId, resolvedId);
        persistAgentSession(restoredAgentId, resolvedId);
      }
      this.sessions = sortSessionsByActivity(this.sessions);
      writeSessionListCache(restoredAgentId, filterSessionsByAgent(restoredAgentId, this.sessions));
      syncDemoChatCache({ sessions: this.sessions });
      return restored;
    },
    async deleteSession(sessionId) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return;
      if (resolveSessionKey(targetId) === resolveSessionKey(this.activeSessionId)) {
        clearSessionWatcher();
      }
      const targetSession = this.sessions.find((item) => item.id === targetId) || null;
      const targetAgentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      abortResumeStream(targetId);
      abortSendStream(targetId);
      setSessionLoading(this, targetId, false);
      this.clearPendingApprovals({ sessionId: targetId });
      sessionRuntime.delete(resolveSessionKey(targetId));
      sessionMessages.delete(resolveSessionKey(targetId));
      sessionDetailWarmState.delete(resolveSessionKey(targetId));
      sessionDetailPrefetchInFlight.delete(resolveSessionKey(targetId));
      sessionSubagentsInFlight.delete(resolveSessionKey(targetId));
      sessionHistoryState.delete(resolveSessionKey(targetId));
      clearSessionCommandSessions(targetId);
      await deleteSessionApi(targetId);
      this.sessions = this.sessions.filter((item) => item.id !== targetId);
      if (targetSession?.is_main) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        const apiAgentId = targetAgentId || DEFAULT_AGENT_KEY;
        if (fallback) {
          await setDefaultSession(apiAgentId, { session_id: fallback.id });
          this.sessions = applyMainSession(this.sessions, targetAgentId, fallback.id);
          persistAgentSession(targetAgentId, fallback.id);
        } else {
          this.sessions = applyMainSession(this.sessions, targetAgentId, '');
          persistAgentSession(targetAgentId, '');
        }
      }
      sessionWorkflowState.delete(String(targetId));
      clearSessionCommandSessions(targetId);
      removeDemoChatSession(targetId);
      clearChatSnapshot(targetId);
      if (resolvePersistedSessionId(targetAgentId) === targetId) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        persistAgentSession(targetAgentId, fallback?.id || '');
      }
      writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
      if (this.activeSessionId === targetId) {
        const nextSession = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        if (nextSession) {
          await this.loadSessionDetail(nextSession.id);
        } else {
          this.openDraftSession({ agent_id: targetAgentId });
        }
      }
    },
    async updateSessionTools(sessionId, toolOverrides = []) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return null;
      const payload = {
        tool_overrides: Array.isArray(toolOverrides) ? toolOverrides : []
      };
      const { data } = await updateSessionToolsApi(targetId, payload);
      const overrides = data?.data?.tool_overrides || [];
      const index = this.sessions.findIndex((item) => item.id === targetId);
      if (index >= 0) {
        this.sessions[index] = {
          ...this.sessions[index],
          tool_overrides: overrides
        };
      }
      return overrides;
    },
    async compactSession(sessionId, payload: Record<string, unknown> = {}) {
      const targetId = String(sessionId || this.activeSessionId || '').trim();
      if (!targetId) {
        throw new Error(t('chat.command.compactMissingSession'));
      }
      const runtime = ensureRuntime(targetId);
      if (runtime) {
        runtime.stopRequested = false;
        if (runtime.compactController) {
          runtime.compactController.abort();
        }
        runtime.compactController = new AbortController();
      }
      const compactController = runtime?.compactController || null;
      const activeSessionId = String(this.activeSessionId || '').trim();
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
        compactionMessage.workflowStreaming = false;
        compactionMessage.reasoningStreaming = false;
        compactionMessage.stream_incomplete = false;
        return data?.data?.message || data?.message || '';
      } catch (error) {
        if (isAbortRequestError(error)) {
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
        compactionMessage.workflowStreaming = false;
        compactionMessage.reasoningStreaming = false;
        compactionMessage.stream_incomplete = false;
        throw error;
      } finally {
        if (runtime && runtime.compactController === compactController) {
          runtime.compactController = null;
        }
        setSessionLoading(this, targetId, false);
        cacheSessionMessages(targetId, targetMessages);
        touchSessionUpdatedAt(this, targetId, Date.now());
        notifySessionSnapshot(this, targetId, targetMessages, true);
      }
    },

    async sendMessage(content: string, options: SendMessageOptions = {}) {
      const initialSessionId = this.activeSessionId;
      abortResumeStream(initialSessionId);
      abortSendStream(initialSessionId);
      abortWatchStream(initialSessionId);
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
      const perfEnabled = chatPerf.enabled();
      const perfStreamStart = perfEnabled ? performance.now() : 0;
      const initialRuntime = ensureRuntime(initialSessionId);
      if (initialRuntime) {
        initialRuntime.stopRequested = false;
      }
      if (!this.activeSessionId) {
        const payload = this.draftAgentId ? { agent_id: this.draftAgentId } : {};
        const session = await this.createSession(payload);
        if (Array.isArray(this.draftToolOverrides)) {
          try {
            await this.updateSessionTools(session.id, this.draftToolOverrides);
          } catch (error) {
            // ignore draft tool override failures
          }
        }
        this.draftToolOverrides = null;
      }
      const attachments = Array.isArray(options.attachments) ? options.attachments : [];
      const userMessage = buildMessage('user', content) as ReturnType<typeof buildMessage> & {
        attachments?: Array<{
          type?: unknown;
          name?: unknown;
          content?: unknown;
          mime_type?: unknown;
        }>;
      };
      if (attachments.length > 0) {
        userMessage.attachments = attachments.map((item) => ({
          type: (item as { type?: unknown })?.type,
          name: (item as { name?: unknown })?.name,
          content: (item as { content?: unknown })?.content,
          mime_type: (item as { mime_type?: unknown })?.mime_type
        }));
      }
      this.messages.push(userMessage);
      const requestStartMs = resolveTimestampMs(userMessage.created_at) ?? Date.now();
      const suppressQueuedNotice = options.suppressQueuedNotice === true;
      const sessionId = this.activeSessionId;
      const runtime = ensureRuntime(sessionId);
      cacheSessionMessages(sessionId, this.messages);
      touchSessionUpdatedAt(this, sessionId, userMessage.created_at);

      const activeSession = this.sessions.find((item) => item.id === sessionId);
      if (activeSession) {
        this.sessions = applyMainSession(this.sessions, activeSession.agent_id, sessionId);
        persistAgentSession(activeSession.agent_id, sessionId);
        if (shouldAutoTitle(activeSession.title)) {
          const autoTitle = buildSessionTitle(content);
          if (autoTitle) {
            activeSession.title = autoTitle;
          }
        }
      }

      const assistantMessageRaw = {
        ...buildMessage('assistant', ''),
        workflowItems: [],
        workflowStreaming: true,
        stream_incomplete: true,
        stream_event_id: 0,
        stream_round: null
      };
      if (assistantMessageRaw.stats) {
        assistantMessageRaw.stats.interaction_start_ms = requestStartMs;
      }
      this.messages.push(assistantMessageRaw);
      const assistantMessage = this.messages[this.messages.length - 1];
      const sessionMessagesRef = this.messages;
      notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);

      setSessionLoading(this, sessionId, true);

      const workflowState = getSessionWorkflowState(sessionId);
      const processor = createWorkflowProcessor(
        assistantMessage,
        workflowState,
        () => notifySessionSnapshot(this, sessionId, sessionMessagesRef),
        {
          streamFlushMs: resolveStreamFlushMsForMessages(sessionMessagesRef),
          sessionId,
          onThreadControl: (payload) => handleThreadControlWorkflowEvent(this, payload),
          onContextUsage: (contextTokens, contextTotalTokens) =>
            syncSessionContextTokens(this, sessionId, contextTokens, contextTotalTokens)
        }
      );
      let queued = false;
      let interruptedByStop = false;
      let finalSeen = false;
      let errorSeen = false;
      let slowClientResumeAfterEventId = 0;

      try {
        if (runtime) {
          clearSlowClientResume(runtime);
          runtime.sendController = new AbortController();
        }
        const desktopToolCallMode = getDesktopToolCallModeForRequest();
        const approvalMode = normalizeApprovalMode(options.approvalMode ?? options.approval_mode);
        const payload = {
          content,
          stream: true,
          ...(attachments.length > 0 ? { attachments } : {}),
          ...(desktopToolCallMode ? { tool_call_mode: desktopToolCallMode } : {}),
          ...(approvalMode ? { approval_mode: approvalMode } : {})
        };
        const onEvent = (eventType, dataText, eventId) => {
          const payload = safeJsonParse(dataText);
          const approvalPayload = payload?.data ?? payload;
          const normalizedEventType = String(eventType || '').trim().toLowerCase();
          handleApprovalEvent(
            this,
            eventType,
            approvalPayload,
            runtime?.sendRequestId || '',
            sessionId
          );
          if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
            updateRuntimeLastEventId(runtime, eventId);
            applySessionRuntimeEvent(this, sessionId, approvalPayload, normalizedEventType);
            return;
          }
          if (perfEnabled) {
            chatPerf.count('chat_stream_event', 1, { eventType, sessionId });
          }
          if (eventType === 'heartbeat' || eventType === 'ping') {
            return;
          }
          const queuedFlag =
            eventType === 'queued' || payload?.queued === true || payload?.data?.queued === true;
          if (queuedFlag) {
            if (!queued) {
              queued = true;
              if (!suppressQueuedNotice) {
                assistantMessage.workflowItems.push(
                  buildWorkflowItem(t('chat.workflow.queued'), t('chat.workflow.queuedDetail'), 'pending')
                );
                notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
              }
            }
            assistantMessage.stream_incomplete = true;
            assistantMessage.workflowStreaming = true;
            return;
          }
          if (eventType === 'final') {
            finalSeen = true;
          } else if (eventType === 'error') {
            errorSeen = true;
          } else if (
            eventType === 'slow_client' &&
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
          if (perfEnabled) {
            const start = performance.now();
            processor.handleEvent(eventType, dataText);
            chatPerf.recordDuration('chat_stream_event_handle', performance.now() - start, {
              eventType,
              sessionId
            });
          } else {
            processor.handleEvent(eventType, dataText);
          }
        };
        const streamWithSse = async () => {
          const response = await sendMessageStream(sessionId, payload, {
            signal: runtime?.sendController?.signal
          });
          if (!response.ok) {
            const errorText = await readResponseError(response);
            throw new Error(
              errorText || t('chat.error.requestFailedWithStatus', { status: response.status })
            );
          }
          await consumeSseStream(response, onEvent);
        };
        const streamWithWs = async () => {
          const requestId = buildWsRequestId();
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
            resolveOnQueued: true
          });
        };
        await this.refreshStreamTransportPolicy();
        const transport = resolveStreamTransport();
        this.streamTransport = transport;
        if (transport === 'ws') {
          try {
            await streamWithWs();
          } catch (error) {
            if (error?.phase === 'connect') {
              markWsUnavailable();
              this.streamTransport = 'sse';
              await streamWithSse();
            } else {
              throw error;
            }
          }
        } else {
          await streamWithSse();
        }
      } catch (error) {
        if (error?.name === 'AbortError' || runtime?.stopRequested || pageUnloading) {
          interruptedByStop = true;
          assistantMessage.reasoningStreaming = false;
          if (!pageUnloading) {
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
        const keepStreaming = !stopped && !terminalSeen;
        const finishedRequestId = runtime?.sendRequestId || '';
        assistantMessage.workflowStreaming = keepStreaming;
        assistantMessage.reasoningStreaming = false;
        assistantMessage.stream_incomplete = keepStreaming;
        if (runtime) {
          runtime.sendController = null;
          runtime.stopRequested = false;
          runtime.sendRequestId = null;
          if (!keepStreaming) {
            clearSlowClientResume(runtime);
          }
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
        if (this.activeSessionId === sessionId && !pageUnloading && keepStreaming) {
          if (slowClientResumeAfterEventId > 0) {
            scheduleSlowClientResume(this, sessionId, assistantMessage, slowClientResumeAfterEventId);
          }
          startSessionWatcher(this, sessionId);
        }
      }
    },
    async stopStream() {
      if (!this.activeSessionId) {
        return false;
      }
      const sessionId = this.activeSessionId;
      const runtime = ensureRuntime(sessionId);
      if (runtime) {
        runtime.stopRequested = true;
      }
      abortSendStream(sessionId);
      abortCompactRequest(sessionId);
      let cancelled = false;
      try {
        const { data } = await cancelMessageStream(sessionId);
        cancelled = Boolean(data?.data?.cancelled);
      } catch (error) {
        // Ignore cancel API failures; local stop behavior still applies.
      }
      const targetMessages =
        String(this.activeSessionId || '').trim() === sessionId ? this.messages : getSessionMessages(sessionId);
      clearSupersededPendingAssistantMessages(targetMessages);
      const pendingAssistant = findPendingAssistantMessage(targetMessages);
      if (pendingAssistant) {
        if (isCompactionMarkerAssistantMessage(pendingAssistant)) {
          finalizeManualCompactionAsCancelled(pendingAssistant);
        } else {
          pendingAssistant.workflowStreaming = false;
          pendingAssistant.reasoningStreaming = false;
          pendingAssistant.stream_incomplete = false;
          pendingAssistant.resume_available = true;
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
      if (Array.isArray(targetMessages)) {
        cacheSessionMessages(sessionId, targetMessages);
        touchSessionUpdatedAt(this, sessionId, Date.now());
        notifySessionSnapshot(this, sessionId, targetMessages, true);
      }
      setSessionLoading(this, sessionId, false);
      return cancelled;
    },
    async resumeStream(sessionId, message, options: ResumeStreamOptions = {}) {
      const force = options.force === true;
      if (!message || (!message.stream_incomplete && !force)) return;
      abortWatchStream(sessionId);
      setSessionLoading(this, sessionId, true);
      const perfEnabled = chatPerf.enabled();
      const perfStreamStart = perfEnabled ? performance.now() : 0;
      message.resume_available = false;
      message.slow_client = false;
      message.workflowStreaming = true;
      message.stream_incomplete = true;
      const sessionMessagesRef = getSessionMessages(sessionId) || this.messages;
      cacheSessionMessages(sessionId, sessionMessagesRef);
      notifySessionSnapshot(this, sessionId, sessionMessagesRef);
      const workflowState = getSessionWorkflowState(sessionId);
      const processor = createWorkflowProcessor(
        message,
        workflowState,
        () => notifySessionSnapshot(this, sessionId, sessionMessagesRef),
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
        runtime.resumeController = new AbortController();
      }
      let aborted = false;
      let finalSeen = false;
      let errorSeen = false;
      let slowClientResumeAfterEventId = 0;
      const forcedEventId = options.afterEventId;
      const normalizedMessageEventId = normalizeStreamEventId(message.stream_event_id);
      const afterEventId = Number.isFinite(Number(forcedEventId))
        ? Number.parseInt(String(forcedEventId), 10)
        : normalizedMessageEventId;
      const resumeAfterEventId = Number.isFinite(afterEventId) ? Math.max(afterEventId, 0) : 0;
      try {
        const onEvent = (eventType, dataText, eventId) => {
          const payload = safeJsonParse(dataText);
          const approvalPayload = payload?.data ?? payload;
          const normalizedEventType = String(eventType || '').trim().toLowerCase();
          if (perfEnabled) {
            chatPerf.count('chat_resume_event', 1, { eventType, sessionId });
          }
          if (eventType === 'heartbeat' || eventType === 'ping') {
            return;
          }
          handleApprovalEvent(
            this,
            eventType,
            approvalPayload,
            runtime?.resumeRequestId || '',
            sessionId
          );
          if (normalizedEventType === 'thread_status' || normalizedEventType === 'thread_closed') {
            updateRuntimeLastEventId(runtime, eventId);
            applySessionRuntimeEvent(this, sessionId, approvalPayload, normalizedEventType);
            return;
          }
          assignStreamEventId(message, eventId);
          updateRuntimeLastEventId(runtime, eventId);
          if (eventType === 'final') {
            finalSeen = true;
          } else if (eventType === 'error') {
            errorSeen = true;
          } else if (
            eventType === 'slow_client' &&
            String(payload?.reason ?? payload?.data?.reason ?? '').trim() === 'queue_full_resume_required'
          ) {
            slowClientResumeAfterEventId = Math.max(
              slowClientResumeAfterEventId,
              getRuntimeLastEventId(runtime),
              normalizeStreamEventId(message.stream_event_id) || 0
            );
          }
          if (perfEnabled) {
            const start = performance.now();
            processor.handleEvent(eventType, dataText);
            chatPerf.recordDuration('chat_resume_event_handle', performance.now() - start, {
              eventType,
              sessionId
            });
          } else {
            processor.handleEvent(eventType, dataText);
          }
        };
        const streamWithSse = async () => {
          const response = await resumeMessageStream(sessionId, {
            signal: runtime?.resumeController?.signal,
            afterEventId: resumeAfterEventId
          });
          if (!response.ok) {
            const errorText = await readResponseError(response);
            throw new Error(
              errorText || t('chat.error.resumeFailedWithStatus', { status: response.status })
            );
          }
          await consumeSseStream(response, onEvent);
        };
        const streamWithWs = async () => {
          const requestId = buildWsRequestId();
          if (runtime) {
            runtime.resumeRequestId = requestId;
          }
          await chatWsClient.request({
            requestId,
            sessionId,
            message: {
              type: 'resume',
              request_id: requestId,
              session_id: sessionId,
              payload: {
                after_event_id: Math.max(afterEventId, 1)
              }
            },
            onEvent,
            signal: runtime?.resumeController?.signal,
            closeOnFinal: true
          });
        };
        const hasAfterEventId = Number.isFinite(afterEventId) && afterEventId > 0;
        if (hasAfterEventId) {
          await this.refreshStreamTransportPolicy();
        }
        const transport = hasAfterEventId ? resolveStreamTransport() : 'sse';
        this.streamTransport = transport;
        if (transport === 'ws') {
          try {
            await streamWithWs();
          } catch (error) {
            if (error?.phase === 'connect') {
              markWsUnavailable();
              this.streamTransport = 'sse';
              await streamWithSse();
            } else {
              throw error;
            }
          }
        } else {
          await streamWithSse();
        }
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
        const keepStreaming = !aborted && !terminalSeen;
        message.workflowStreaming = keepStreaming;
        if (!aborted) {
          message.stream_incomplete = keepStreaming;
        }
        if (runtime) {
          runtime.resumeController = null;
          runtime.resumeRequestId = null;
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
        if (!aborted && this.activeSessionId === sessionId && !pageUnloading && keepStreaming) {
          if (slowClientResumeAfterEventId > 0) {
            scheduleSlowClientResume(this, sessionId, message, slowClientResumeAfterEventId);
          }
          startSessionWatcher(this, sessionId);
        }
      }
    }
  }
});
