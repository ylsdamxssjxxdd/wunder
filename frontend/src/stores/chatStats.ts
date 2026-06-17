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
import { parseWorkspaceResourceUrl } from '@/utils/workspaceResources';
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

import { findAssistantMessageByRound, findAssistantMessageByUserRound } from './chatMessageLookup';
import { normalizeStreamRound } from './chatStreamIds';
import { MessageSubagentItem, NormalizedUsagePayload } from './chatTypes';

export const buildMessageStats = () => ({
  toolCalls: 0,
  usage: null,
  roundUsage: null,
  prefill_duration_s: null,
  decode_duration_s: null,
  prefill_duration_total_s: null,
  decode_duration_total_s: null,
  avg_model_round_speed_tps: null,
  avg_model_round_speed_rounds: 0,
  quotaConsumed: 0,
  partialQuotaConsumed: 0,
  quotaSnapshot: null,
  contextTokens: null,
  contextPreviewTokens: null,
  contextTotalTokens: null,
  interaction_start_ms: null,
  interaction_end_ms: null,
  interaction_duration_s: null
});

export const normalizeStatsCount = (value) => {
  if (value === null || value === undefined) return 0;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 0;
};

export const parseOptionalCount = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

export const parseWorkflowUsageRecord = (value, depth = 0) => {
  if (depth > 3 || value === null || value === undefined) {
    return null;
  }
  if (typeof value === 'object' && !Array.isArray(value)) {
    return value;
  }
  if (typeof value !== 'string') {
    return null;
  }
  const normalized = value.trim();
  if (!normalized || normalized.length < 2 || (normalized[0] !== '{' && normalized[0] !== '[')) {
    return null;
  }
  try {
    const parsed = JSON.parse(normalized);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : null;
  } catch {
    return null;
  }
};

export const WORKFLOW_USAGE_DIRECT_KEYS = [
  'request_consumed_tokens',
  'requestConsumedTokens',
  'consumed_tokens',
  'consumedTokens',
  'consumed',
  'used',
  'count',
  'context_occupancy_tokens',
  'contextOccupancyTokens',
  'context_tokens',
  'contextTokens',
  'context_total_tokens',
  'contextTotalTokens',
  'max_context',
  'maxContext'
];

export const WORKFLOW_USAGE_NESTED_KEYS = [
  'usage',
  'roundUsage',
  'round_usage',
  'billedUsage',
  'billed_usage',
  'quotaConsumed',
  'quota_consumed',
  'quota',
  'stats',
  'meta',
  'data',
  'result',
  'payload'
];

export const buildWorkflowUsageSnapshot = (value, depth = 0) => {
  const record = parseWorkflowUsageRecord(value, depth);
  if (!record) {
    return null;
  }
  const snapshot = {};
  WORKFLOW_USAGE_DIRECT_KEYS.forEach((key) => {
    if (record[key] !== undefined) {
      snapshot[key] = record[key];
    }
  });
  WORKFLOW_USAGE_NESTED_KEYS.forEach((key) => {
    const nested = buildWorkflowUsageSnapshot(record[key], depth + 1);
    if (nested && Object.keys(nested).length > 0) {
      snapshot[key] = nested;
    }
  });
  return Object.keys(snapshot).length > 0 ? snapshot : null;
};

export const buildWorkflowUsageMeta = (...sources) => {
  for (const source of sources) {
    const snapshot = buildWorkflowUsageSnapshot(source);
    if (snapshot && Object.keys(snapshot).length > 0) {
      // Preserve a compact raw usage snapshot so UI token display does not depend
      // on human-facing model_observation/detail strings.
      return { payload: snapshot };
    }
  }
  return {};
};

export const buildWorkflowTimingMeta = (...sources) => {
  const durationMs = resolveWorkflowDurationMs(...sources);
  return durationMs !== null ? { durationMs } : {};
};

export const hasWorkflowUsageConsumedTokens = (value, depth = 0) => {
  if (depth > 3) {
    return false;
  }
  const record = parseWorkflowUsageRecord(value, depth);
  if (!record) {
    return false;
  }
  const explicitConsumed = normalizeStatsCount(
    record.request_consumed_tokens ??
      record.requestConsumedTokens ??
      record.consumed_tokens ??
      record.consumedTokens ??
      record.consumed ??
      record.used ??
      record.count
  );
  if (explicitConsumed > 0) {
    return true;
  }
  const usageConsumed = normalizeStatsCount(
    record.total ??
      record.total_tokens ??
      record.totalTokens ??
      record.input ??
      record.input_tokens ??
      record.inputTokens
  );
  if (usageConsumed > 0) {
    return true;
  }
  return WORKFLOW_USAGE_NESTED_KEYS.some((key) =>
    hasWorkflowUsageConsumedTokens(record[key], depth + 1)
  );
};

export const mergeWorkflowUsageSnapshot = (current, incoming) => {
  const base = parseWorkflowUsageRecord(current);
  const patch = parseWorkflowUsageRecord(incoming);
  if (!base) {
    return patch ?? null;
  }
  if (!patch) {
    return base;
  }
  const merged = { ...base };
  Object.entries(patch).forEach(([key, value]) => {
    const existing = merged[key];
    const existingRecord = parseWorkflowUsageRecord(existing);
    const nextRecord = parseWorkflowUsageRecord(value);
    if (existingRecord && nextRecord) {
      merged[key] = mergeWorkflowUsageSnapshot(existingRecord, nextRecord);
      return;
    }
    if (existing === undefined || existing === null || existing === '') {
      merged[key] = value;
      return;
    }
    if (!hasWorkflowUsageConsumedTokens(existing) && hasWorkflowUsageConsumedTokens(value)) {
      merged[key] = value;
    }
  });
  return merged;
};

export const buildWorkflowModelRoundUsageMeta = (...sources) => {
  for (const source of sources) {
    const record = parseWorkflowUsageRecord(source);
    if (!record) {
      continue;
    }
    const usageSource = parseWorkflowUsageRecord(record.usage) ?? record;
    const usage: Record<string, number> = {};
    const inputTokens = parseOptionalCount(
      usageSource.input_tokens ?? usageSource.input ?? usageSource.inputTokens
    );
    const outputTokens = parseOptionalCount(
      usageSource.output_tokens ?? usageSource.output ?? usageSource.outputTokens
    );
    const totalTokens = parseOptionalCount(
      usageSource.total_tokens ?? usageSource.total ?? usageSource.totalTokens
    );
    if (inputTokens !== null) {
      usage.input_tokens = inputTokens;
    }
    if (outputTokens !== null) {
      usage.output_tokens = outputTokens;
    }
    if (totalTokens !== null) {
      usage.total_tokens = totalTokens;
    }
    if (Object.keys(usage).length > 0) {
      return { payload: { usage } };
    }
  }
  return {};
};

export const combineWorkflowUsageMeta = (...sources) => {
  let merged = null;
  sources.forEach((source) => {
    const payload = parseWorkflowUsageRecord(source?.payload ?? source);
    if (!payload) {
      return;
    }
    merged = mergeWorkflowUsageSnapshot(merged, payload);
  });
  return merged ? { payload: merged } : {};
};

export const summarizeWorkflowUsageDebug = (value, depth = 0) => {
  if (depth > 2) {
    return null;
  }
  const record = parseWorkflowUsageRecord(value, depth);
  if (!record) {
    return null;
  }
  const explicitConsumed = normalizeStatsCount(
    record.request_consumed_tokens ??
      record.requestConsumedTokens ??
      record.consumed_tokens ??
      record.consumedTokens ??
      record.consumed ??
      record.used ??
      record.count
  );
  const usageTotal = normalizeStatsCount(
    record.total ?? record.total_tokens ?? record.totalTokens
  );
  const usageInput = normalizeStatsCount(
    record.input ?? record.input_tokens ?? record.inputTokens
  );
  const contextTokens = normalizeStatsCount(
    record.context_occupancy_tokens ??
      record.contextOccupancyTokens ??
      record.context_tokens ??
      record.contextTokens
  );
  const roundUsage =
    summarizeWorkflowUsageDebug(record.roundUsage ?? record.round_usage, depth + 1) ?? undefined;
  const usage = summarizeWorkflowUsageDebug(record.usage, depth + 1) ?? undefined;
  const summary = {
    explicitConsumed: explicitConsumed > 0 ? explicitConsumed : null,
    usageTotal: usageTotal > 0 ? usageTotal : null,
    usageInput: usageInput > 0 ? usageInput : null,
    contextTokens: contextTokens > 0 ? contextTokens : null,
    roundUsage,
    usage
  };
  return Object.values(summary).some((item) => item !== null && item !== undefined) ? summary : null;
};

export const WORKSPACE_PATH_HINT_KEYS = [
  'path',
  'paths',
  'changed_paths',
  'changedPaths',
  'public_path',
  'publicPath',
  'workspace_relative_path',
  'workspaceRelativePath',
  'target_path',
  'targetPath',
  'source_path',
  'sourcePath',
  'destination',
  'destination_path',
  'destinationPath',
  'output_path',
  'outputPath',
  'saved_path',
  'savedPath',
  'file_path',
  'filePath',
  'file',
  'files',
  'relative_path',
  'relativePath',
  'to_path'
];

export const normalizeWorkspaceEventPath = (value) => {
  const text = String(value || '').trim();
  if (!text || text === '/' || text === '.') return '';
  const parsed = parseWorkspaceResourceUrl(text);
  if (parsed?.relativePath) return parsed.relativePath.replace(/\\/g, '/').replace(/^\/+/, '');
  const normalized = text.replace(/\\/g, '/').replace(/^\/+/, '');
  return normalized === '.' ? '' : normalized;
};

// Extract path hints from heterogeneous tool/workspace payloads for incremental UI refresh.
export const collectWorkspacePathHints = (...sources) => {
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
        record.public_path ??
        record.publicPath ??
        record.workspace_relative_path ??
        record.workspaceRelativePath ??
        record.relative_path ??
        record.relativePath ??
        record.target_path ??
        record.targetPath ??
        record.source_path ??
        record.sourcePath ??
        record.destination ??
        record.destination_path ??
        record.destinationPath ??
        record.output_path ??
        record.outputPath ??
        record.saved_path ??
        record.savedPath ??
        record.file_path ??
        record.filePath ??
        record.to_path
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

export const normalizeQuotaConsumed = (value) => {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return normalizeStatsCount(
      value.request_consumed_tokens ??
        value.requestConsumedTokens ??
        value.consumed_tokens ??
        value.consumedTokens ??
        value.consumed ??
        value.used ??
        value.count ??
        0
    );
  }
  return normalizeStatsCount(value);
};

export const resolveUsageConsumedTokensFromPayload = (value) => {
  const usage = normalizeUsagePayload(value);
  if (!usage) return 0;
  return normalizeStatsCount(usage.total ?? usage.input);
};

export const normalizeQuotaSnapshot = (value) => {
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

export const normalizeContextTokens = (value) => parseOptionalCount(value);
export const normalizeContextTotalTokens = (value) => {
  const normalized = parseOptionalCount(value);
  if (normalized === null || normalized <= 0) return null;
  return normalized;
};

export const resolveContextUsageRecord = (source) => {
  if (!source || typeof source !== 'object') return null;
  const record = source as Record<string, unknown>;
  const value = record.context_usage ?? record.contextUsage;
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
};

export const resolveExplicitContextTokens = (source) => {
  if (!source || typeof source !== 'object') return null;
  const record = source as Record<string, unknown>;
  const contextUsage = resolveContextUsageRecord(record);
  return normalizeContextTokens(
    record.context_occupancy_tokens ??
      record.contextOccupancyTokens ??
      contextUsage?.context_occupancy_tokens ??
      contextUsage?.contextOccupancyTokens ??
      record.contextTokens ??
      record.context_tokens ??
      contextUsage?.contextTokens ??
      contextUsage?.context_tokens
  );
};

export const resolveContextPreviewTokens = (source) => {
  if (!source || typeof source !== 'object') return null;
  const record = source as Record<string, unknown>;
  return normalizeContextTokens(
    record.contextPreviewTokens ??
      record.context_preview_tokens ??
      record.contextEstimateTokens ??
      record.context_estimate_tokens ??
      record.estimatedContextTokens ??
      record.estimated_context_tokens
  );
};

export const resolveExplicitContextTotalTokens = (source) => {
  if (!source || typeof source !== 'object') return null;
  const record = source as Record<string, unknown>;
  const contextUsage = resolveContextUsageRecord(record);
  return normalizeContextTotalTokens(
    record.contextTotalTokens ??
      record.context_total_tokens ??
      record.context_max_tokens ??
      record.context_window ??
      record.max_context ??
      record.maxContext ??
      record.contextUsageTotal ??
      contextUsage?.max_context ??
      contextUsage?.maxContext ??
      contextUsage?.context_max_tokens ??
      contextUsage?.contextMaxTokens
  );
};

export const normalizeDurationValue = (value) => {
  return normalizeChatDurationSeconds(value);
};

export const normalizeSpeedValue = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const resolveInteractionDuration = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return null;
  }
  return (endMs - startMs) / 1000;
};

export const normalizeInteractionTimestamp = (value) => {
  const millis = resolveTimestampMs(value);
  return Number.isFinite(millis) ? millis : null;
};

export const normalizeSubagentEventStatus = (value) => {
  const normalized = normalizeSubagentRuntimeStatus(value);
  if (!normalized) return 'running';
  return normalized;
};

export const pickSubagentTextValue = (...values: unknown[]): string => {
  for (const value of values) {
    const text = String(value || '').trim();
    if (text) return text;
  }
  return '';
};

export const unwrapSubagentDetailPayload = (value: unknown): Record<string, unknown> => {
  let cursor =
    value && typeof value === 'object' && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  for (let depth = 0; depth < 8; depth += 1) {
    const nested = cursor.detail;
    if (!nested || typeof nested !== 'object' || Array.isArray(nested)) {
      break;
    }
    const nestedRecord = nested as Record<string, unknown>;
    const nestedSession = String(nestedRecord.session_id ?? '').trim();
    const nestedRun = String(nestedRecord.run_id ?? '').trim();
    const currentSession = String(cursor.session_id ?? '').trim();
    const currentRun = String(cursor.run_id ?? '').trim();
    const sameIdentity =
      (nestedSession && currentSession && nestedSession === currentSession) ||
      (nestedRun && currentRun && nestedRun === currentRun);
    if (!sameIdentity && depth > 0) {
      break;
    }
    cursor = nestedRecord;
  }
  const flattened = { ...cursor };
  delete flattened.detail;
  return flattened;
};

export const sanitizeSubagentDetailPayload = (
  detail: Record<string, unknown>,
  source: Record<string, unknown>,
  agentState: Record<string, unknown>
): Record<string, unknown> => {
  const detailAgentState =
    detail.agent_state && typeof detail.agent_state === 'object' && !Array.isArray(detail.agent_state)
      ? (detail.agent_state as Record<string, unknown>)
      : {};
  const mergedAgentState = { ...detailAgentState, ...agentState };
  const mergedAgentStatus = normalizeSubagentEventStatus(mergedAgentState.status ?? detail.status ?? source.status);

  const assistantMessage = pickSubagentTextValue(
    detail.assistant_message,
    detail.assistantMessage,
    source.assistant_message,
    source.assistantMessage,
    detail.result,
    source.result,
    detail.message,
    source.message,
    mergedAgentState.message
  );
  const errorMessage = pickSubagentTextValue(
    detail.error_message,
    detail.errorMessage,
    source.error_message,
    source.errorMessage,
    detail.error,
    source.error
  );
  const sessionId = pickSubagentTextValue(
    detail.session_id,
    detail.sessionId,
    source.session_id,
    source.sessionId
  );
  const runId = pickSubagentTextValue(detail.run_id, detail.runId, source.run_id, source.runId);
  const agentId = pickSubagentTextValue(
    detail.agent_id,
    detail.agentId,
    source.agent_id,
    source.agentId
  );
  const modelName = pickSubagentTextValue(
    detail.model_name,
    detail.modelName,
    source.model_name,
    source.modelName
  );
  const requestedBy = pickSubagentTextValue(
    detail.requested_by,
    detail.requestedBy,
    source.requested_by,
    source.requestedBy
  );
  const spawnedBy = pickSubagentTextValue(
    detail.spawned_by,
    detail.spawnedBy,
    source.spawned_by,
    source.spawnedBy
  );
  const elapsedSeconds = normalizeDurationValue(
    detail.elapsed_s ?? detail.elapsedSeconds ?? source.elapsed_s ?? source.elapsedSeconds
  );
  const queuedAt = resolveTimestampIso(
    detail.queued_time ?? detail.queuedTime ?? source.queued_time ?? source.queuedTime
  );
  const startedAt = resolveTimestampIso(
    detail.started_time ?? detail.startedTime ?? source.started_time ?? source.startedTime
  );
  const finishedAt = resolveTimestampIso(
    detail.finished_time ?? detail.finishedTime ?? source.finished_time ?? source.finishedTime
  );
  const updatedAt = resolveTimestampIso(
    detail.updated_time ??
      detail.updatedTime ??
      source.updated_time ??
      source.updatedTime ??
      source.finished_time ??
      source.finishedTime
  );

  const compacted: Record<string, unknown> = {};
  if (agentId) compacted.agent_id = agentId;
  if (sessionId) compacted.session_id = sessionId;
  if (runId) compacted.run_id = runId;
  if (mergedAgentStatus) compacted.status = mergedAgentStatus;
  if (assistantMessage) compacted.assistant_message = assistantMessage;
  if (errorMessage && errorMessage !== assistantMessage) compacted.error = errorMessage;
  if (modelName) compacted.model_name = modelName;
  if (Number.isFinite(elapsedSeconds) && elapsedSeconds >= 0) compacted.elapsed_s = elapsedSeconds;
  if (queuedAt) compacted.queued_at = queuedAt;
  if (startedAt) compacted.started_at = startedAt;
  if (finishedAt) compacted.finished_at = finishedAt;
  if (updatedAt) compacted.updated_at = updatedAt;
  if (requestedBy) compacted.requested_by = requestedBy;
  if (spawnedBy) compacted.spawned_by = spawnedBy;
  return compacted;
};

export const normalizeMessageSubagent = (payload): MessageSubagentItem | null => {
  if (!payload || typeof payload !== 'object') return null;
  const source = payload as Record<string, unknown>;
  const rawDetail =
    source.detail && typeof source.detail === 'object' && !Array.isArray(source.detail)
      ? (source.detail as Record<string, unknown>)
      : source;
  const agentState =
    source.agent_state && typeof source.agent_state === 'object'
      ? (source.agent_state as Record<string, unknown>)
      : source.agentState && typeof source.agentState === 'object'
        ? (source.agentState as Record<string, unknown>)
        : {};
  const normalizedDetail = sanitizeSubagentDetailPayload(
    unwrapSubagentDetailPayload(rawDetail),
    source,
    agentState
  );
  const sessionId = String(source.session_id ?? source.sessionId ?? '').trim();
  const runId = String(source.run_id ?? source.runId ?? '').trim();
  const key = runId || sessionId;
  if (!key) return null;
  const label = String(
    source.label ?? source.spawn_label ?? source.spawnLabel ?? source.title ?? ''
  ).trim();
  const title = label || sessionId || runId || '子智能体';
  const status = normalizeSubagentEventStatus(source.status);
  const active = isSubagentItemActive(source);
  const assistantMessage = pickSubagentTextValue(
    normalizedDetail.assistant_message,
    source.assistant_message,
    source.result,
    agentState.message
  );
  const summary = String(
    source.summary ?? assistantMessage ?? normalizedDetail.error ?? source.error ?? ''
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
  const agentStateMessageRaw = pickSubagentTextValue(agentState.message, assistantMessage);
  const agentStateMessage =
    agentStateMessageRaw && agentStateMessageRaw !== assistantMessage ? agentStateMessageRaw : '';
  return {
    key,
    session_id: sessionId,
    run_id: runId,
    dispatch_id: String(source.dispatch_id ?? source.dispatchId ?? '').trim(),
    title,
    label,
    status,
    summary,
    terminal:
      normalizeSubagentRuntimeFlag(source.terminal) ||
      (!active && (isSubagentStatusSuccessful(status) || isSubagentStatusFailed(status))),
    failed: normalizeSubagentRuntimeFlag(source.failed) || isSubagentStatusFailed(status),
    canTerminate: normalizeSubagentRuntimeFlag(source.can_terminate ?? source.canTerminate ?? active),
    updated_at: updatedAt,
    updated_at_ms: updatedAtMs,
    parent_user_round: normalizeStreamRound(
      source.parent_user_round ?? source.parentUserRound
    ),
    parent_model_round: normalizeStreamRound(
      source.parent_model_round ?? source.parentModelRound
    ),
    detail: normalizedDetail,
    agent_state: {
      status: agentStateStatus,
      message: agentStateMessage
    }
  };
};

export const normalizeMessageSubagents = (items): MessageSubagentItem[] => {
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

export const upsertMessageSubagent = (message, payload) => {
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

export const resolveSubagentPayloadItems = (source: unknown): unknown[] => {
  if (!source || typeof source !== 'object' || Array.isArray(source)) return [];
  const record = source as Record<string, unknown>;
  const candidates = [
    record.item,
    record.selected_item,
    record.selectedItem,
    record.winner_item,
    record.winnerItem,
    ...(Array.isArray(record.items) ? record.items : []),
    ...(Array.isArray(record.selected_items) ? record.selected_items : []),
    ...(Array.isArray(record.selectedItems) ? record.selectedItems : []),
    ...(Array.isArray(record.settled_items) ? record.settled_items : []),
    ...(Array.isArray(record.settledItems) ? record.settledItems : [])
  ];
  if (record.data && typeof record.data === 'object' && !Array.isArray(record.data)) {
    candidates.push(...resolveSubagentPayloadItems(record.data));
  }
  return candidates.filter((item) => item && typeof item === 'object');
};

export const hasSubagentIdentity = (source: unknown): boolean => {
  if (!source || typeof source !== 'object' || Array.isArray(source)) return false;
  const record = source as Record<string, unknown>;
  return Boolean(String(record.session_id ?? record.sessionId ?? record.run_id ?? record.runId ?? '').trim());
};

export const collectSubagentPayloads = (source: unknown): Record<string, unknown>[] => {
  const output: Record<string, unknown>[] = [];
  const append = (item: unknown) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)) return;
    const record = item as Record<string, unknown>;
    if (hasSubagentIdentity(record)) {
      output.push(record);
    }
    resolveSubagentPayloadItems(record).forEach(append);
  };
  append(source);
  return output;
};

export const attachSubagentsToMessages = (messages, subagents) => {
  if (!Array.isArray(messages) || messages.length === 0) {
    return messages;
  }
  const normalized = normalizeMessageSubagents(subagents);
  const previousOwnerByKey = new Map<string, Record<string, unknown>>();
  const assistantMessages = messages.filter((message) => message?.role === 'assistant');
  assistantMessages.forEach((message) => {
    normalizeMessageSubagents(message.subagents).forEach((item) => {
      previousOwnerByKey.set(item.key, message);
    });
  });
  const fallbackTarget =
    [...assistantMessages]
      .reverse()
      .find((message) => {
        const status = String(message?.status || '').trim().toLowerCase();
        const stopReason = String(message?.stop_reason ?? message?.stopReason ?? '').trim().toLowerCase();
        return !(
          message?.cancelled === true ||
          status === 'cancelled' ||
          status === 'canceled' ||
          stopReason === 'user_stop'
        );
      }) ||
    [...assistantMessages].reverse()[0] ||
    null;
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
      previousOwnerByKey.get(item.key) ||
      fallbackTarget;
    if (!target) return;
    upsertMessageSubagent(target, item);
  });
  return messages;
};

export const normalizeUsagePayload = (payload) => {
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

export const estimateStreamOutputTokens = estimateChatTextTokens;

export const normalizeMessageStats = (stats) => {
  if (!stats || typeof stats !== 'object') {
    return null;
  }
  const normalizedUsage = normalizeUsagePayload(stats.usage ?? stats.tokenUsage ?? stats.token_usage);
  const normalizedRoundUsage = normalizeUsagePayload(
    stats.roundUsage ?? stats.round_usage ?? stats.round_usage_total ?? stats.billedUsage
  );
  const explicitContextTokens = resolveExplicitContextTokens(stats);
  const contextPreviewTokens = resolveContextPreviewTokens(stats);
  const quotaSnapshot = normalizeQuotaSnapshot(
    stats.quotaSnapshot ?? stats.quota ?? stats.quota_usage ?? stats.quotaUsage
  );
  const contextTokens = explicitContextTokens;
  const contextTotalTokens = resolveExplicitContextTotalTokens(stats);
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
    roundUsage: normalizedRoundUsage,
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
        stats.avg_model_round_decode_speed_tps ??
        stats.avgModelRoundDecodeSpeedTps ??
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
      stats.quotaConsumed ??
        stats.quota_consumed ??
        stats.request_consumed_tokens ??
        stats.requestConsumedTokens ??
        stats.consumed_tokens ??
        stats.consumedTokens ??
        stats.quota
    ),
    partialQuotaConsumed: normalizeQuotaConsumed(
      stats.partialQuotaConsumed ??
        stats.partial_quota_consumed ??
        stats.partialConsumedTokens ??
        stats.partial_consumed_tokens
    ),
    quotaSnapshot,
    contextTokens,
    contextPreviewTokens,
    contextTotalTokens,
    interaction_start_ms: interactionStartMs,
    interaction_end_ms: interactionEndMs,
    interaction_duration_s: rangedDuration ?? interactionDuration
  };
};

export const parseErrorText = (text) => {
  if (!text) {
    return '';
  }
  return formatStructuredErrorText(text, text);
};

export const ensureMessageStats = (message) => {
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

export const mergeMessageStats = (base, incoming) => {
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
  const incomingExplicitContextTokens = resolveExplicitContextTokens(incoming);
  const incomingPreviewContextTokens = resolveContextPreviewTokens(incoming);
  const contextTokens =
    incomingExplicitContextTokens === null || incomingExplicitContextTokens === undefined
      ? left.contextTokens
      : incomingExplicitContextTokens;
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
    roundUsage: right.roundUsage || left.roundUsage,
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
    partialQuotaConsumed: Math.max(left.partialQuotaConsumed, right.partialQuotaConsumed),
    quotaSnapshot,
    contextTokens,
    contextPreviewTokens:
      incomingPreviewContextTokens === null || incomingPreviewContextTokens === undefined
        ? left.contextPreviewTokens
        : incomingPreviewContextTokens,
    contextTotalTokens,
    interaction_start_ms: startMs,
    interaction_end_ms: endMs,
    interaction_duration_s: duration
  };
};

export const resolveTimestampMs = (value) => {
  return normalizeChatTimestampMs(value);
};

export const resolveTimestampIso = (value) => {
  const millis = resolveTimestampMs(value);
  return millis === null ? '' : new Date(millis).toISOString();
};

export const buildMessage = (role, content, createdAt = undefined, extra = undefined) => ({
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

export const touchAssistantWaitingActivity = (message, value = Date.now()) => {
  if (!message || message.role !== 'assistant') return null;
  const millis = normalizeInteractionTimestamp(value) ?? Date.now();
  message.waiting_updated_at_ms = millis;
  return millis;
};

export const resetAssistantWaitingOutputPhase = (message, value = Date.now()) => {
  const millis = touchAssistantWaitingActivity(message, value) ?? Date.now();
  message.waiting_phase_first_output_at_ms = null;
  return millis;
};

export const markAssistantWaitingOutputVisible = (message, value = Date.now()) => {
  const millis = touchAssistantWaitingActivity(message, value) ?? Date.now();
  if (!Number.isFinite(Number(message?.waiting_phase_first_output_at_ms))) {
    message.waiting_phase_first_output_at_ms = millis;
  }
  if (!Number.isFinite(Number(message?.waiting_first_output_at_ms))) {
    message.waiting_first_output_at_ms = millis;
  }
  return millis;
};

export const clearAssistantRetryState = (message) => {
  if (!message || message.role !== 'assistant') return;
  delete message.retry_state;
  delete message.retry_attempt;
  delete message.retry_max_attempts;
  delete message.retry_delay_s;
  delete message.retry_started_at_ms;
  delete message.retry_next_attempt_at_ms;
  delete message.retry_reason;
  delete message.retry_error;
};

export const markAssistantRetryState = (
  message,
  options: {
    attempt?: number | null;
    maxAttempts?: number | null;
    delayS?: number | null;
    startedAtMs?: number | null;
    reason?: string;
    error?: string;
  } = {}
) => {
  if (!message || message.role !== 'assistant') return;
  const startedAtMs = normalizeInteractionTimestamp(options.startedAtMs) ?? Date.now();
  const attempt = parseOptionalCount(options.attempt);
  const maxAttempts = parseOptionalCount(options.maxAttempts);
  const delayS = Number.isFinite(Number(options.delayS)) && Number(options.delayS) > 0
    ? Number(options.delayS)
    : null;
  message.retry_state = 'retrying';
  message.retry_attempt = attempt;
  message.retry_max_attempts = maxAttempts;
  message.retry_delay_s = delayS;
  message.retry_started_at_ms = startedAtMs;
  message.retry_next_attempt_at_ms = delayS !== null ? startedAtMs + delayS * 1000 : null;
  message.retry_reason = String(options.reason || '').trim();
  message.retry_error = String(options.error || '').trim();
};

export const normalizeHiddenInternalMessage = (value) => Boolean(value);

export const resolveGreetingContent = (override) => {
  const trimmed = String(override || '').trim();
  return trimmed ? trimmed : t('chat.greeting');
};
