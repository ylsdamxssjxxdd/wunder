import { normalizeChatDurationSeconds } from './chatTiming';
import { resolveAssistantFailureNotice } from './assistantFailureNotice';
import { isCompactionRunningFromWorkflowItems } from './chatCompactionWorkflow';

export type MessageStatsEntry = {
  key: string;
  label: string;
  value: string;
  kind?: 'status' | 'metric';
  tone?: 'running' | 'warning' | 'success' | 'error' | 'muted';
  live?: boolean;
};

type TranslateFn = (key: string) => string;
type WorkflowItemLike = Record<string, any>;

const formatDuration = (seconds: unknown): string => {
  if (seconds === null || seconds === undefined || Number.isNaN(Number(seconds))) return '-';
  const value = Number(seconds);
  if (!Number.isFinite(value) || value < 0) return '-';
  if (value < 1) {
    return `${Math.max(1, Math.round(value * 1000))} ms`;
  }
  return `${value.toFixed(2)} s`;
};

const formatCount = (value: unknown): string => {
  if (value === null || value === undefined) return '-';
  const parsed = Number.parseInt(String(value), 10);
  if (!Number.isFinite(parsed) || parsed < 0) return '-';
  return String(parsed);
};

const formatSpeed = (value: unknown): string => {
  if (value === null || value === undefined) return '-';
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return '-';
  return `${parsed.toFixed(2)} token/s`;
};

const normalizeSpeed = (speed: number): number | null => {
  if (!Number.isFinite(speed) || speed <= 0) return null;
  return speed;
};

const normalizeDurationSeconds = (value: unknown): number | null => {
  return normalizeChatDurationSeconds(value);
};

const resolveDurationSeconds = (stats: Record<string, any>): number | null => {
  const interaction = normalizeDurationSeconds(
    stats?.interaction_duration_s ??
      stats?.interactionDurationS ??
      stats?.interactionDuration ??
      stats?.duration_s ??
      stats?.elapsed_s
  );
  if (interaction !== null) return interaction;
  const prefill = normalizeDurationSeconds(stats?.prefill_duration_s);
  const decode = normalizeDurationSeconds(stats?.decode_duration_s);
  if (prefill === null && decode === null) return null;
  return (prefill ?? 0) + (decode ?? 0);
};

const resolveTokenSpeed = (stats: Record<string, any>): number | null => {
  const averageSpeed = normalizeSpeed(
    Number(
      stats?.avg_model_round_speed_tps ??
        stats?.avg_model_round_decode_speed_tps ??
        stats?.avgModelRoundDecodeSpeedTps ??
        stats?.avgModelRoundSpeedTps ??
        stats?.average_speed_tps ??
        stats?.averageSpeedTps
    )
  );
  const averageRounds = Number(
    stats?.avg_model_round_speed_rounds ??
      stats?.avgModelRoundSpeedRounds ??
      stats?.average_speed_rounds ??
      stats?.averageSpeedRounds
  );
  return averageSpeed !== null && (!Number.isFinite(averageRounds) || averageRounds > 0)
    ? averageSpeed
    : null;
};

const isAssistantStreaming = (message: Record<string, any>): boolean =>
  Boolean(message?.stream_incomplete || message?.workflowStreaming || message?.reasoningStreaming);

const hasAssistantVisibleOutput = (message: Record<string, any>): boolean =>
  Boolean(String(message?.content || '').trim()) || Boolean(String(message?.reasoning || '').trim());

const hasAssistantWaitingForCurrentOutput = (message: Record<string, any>): boolean => {
  const waitingUpdatedAtMs = Number(
    message?.waiting_updated_at_ms ?? message?.waitingUpdatedAtMs ?? message?.stats?.interaction_start_ms
  );
  const waitingFirstOutputAtMs = Number(
    message?.waiting_phase_first_output_at_ms ??
      message?.waitingPhaseFirstOutputAtMs ??
      message?.waiting_first_output_at_ms ??
      message?.waitingFirstOutputAtMs
  );
  return (
    Number.isFinite(waitingUpdatedAtMs) &&
    waitingUpdatedAtMs > 0 &&
    (!Number.isFinite(waitingFirstOutputAtMs) || waitingFirstOutputAtMs <= 0)
  );
};

const normalizeWorkflowEventType = (value: unknown): string => String(value || '').trim().toLowerCase();
const normalizeWorkflowStatus = (value: unknown): string => String(value || '').trim().toLowerCase();

const ACTIVE_WORKFLOW_STATUSES = new Set(['loading', 'pending', 'running', 'streaming']);

const isToolWorkflowItem = (item: WorkflowItemLike): boolean => {
  if (!item || typeof item !== 'object') return false;
  if (item?.isTool) return true;
  const eventType = normalizeWorkflowEventType(item?.eventType ?? item?.event);
  if (!eventType) return false;
  return (
    eventType.startsWith('tool_') ||
    eventType.startsWith('subagent_') ||
    eventType.startsWith('team_') ||
    eventType.startsWith('command_session_')
  );
};

const isModelOutputWorkflowItem = (item: WorkflowItemLike): boolean => {
  if (!item || typeof item !== 'object') return false;
  if (item?.isModelOutput) return true;
  return normalizeWorkflowEventType(item?.eventType ?? item?.event) === 'llm_output';
};

const findLastWorkflowItem = (
  items: WorkflowItemLike[],
  predicate: (item: WorkflowItemLike, index: number) => boolean
): { item: WorkflowItemLike | null; index: number } => {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (!item || typeof item !== 'object') continue;
    if (predicate(item, index)) {
      return { item, index };
    }
  }
  return { item: null, index: -1 };
};

const hasPendingQuestionPanel = (message: Record<string, any>): boolean => {
  const panelStatus = String(message?.questionPanel?.status || '').trim().toLowerCase();
  return (
    panelStatus === 'pending' ||
    Boolean(message?.pendingQuestion) ||
    Boolean(message?.pending_question) ||
    Boolean(message?.awaiting_confirmation) ||
    Boolean(message?.requires_confirmation)
  );
};

const hasAssistantActivitySignals = (message: Record<string, any> | null | undefined): boolean => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return false;
  return Boolean(
    hasPendingQuestionPanel(message) ||
      isAssistantStreaming(message) ||
      message?.resume_available ||
      message?.slow_client ||
      (Array.isArray(message?.workflowItems) && message.workflowItems.length > 0) ||
      (message?.stats && typeof message.stats === 'object')
  );
};

const buildStatusEntry = (
  value: string,
  tone: NonNullable<MessageStatsEntry['tone']>,
  live = false
): MessageStatsEntry => ({
  key: 'status',
  label: '',
  value,
  kind: 'status',
  tone,
  live
});

const resolveAssistantStatusEntry = (
  message: Record<string, any>,
  t: TranslateFn
): MessageStatsEntry | null => {
  if (!hasAssistantActivitySignals(message)) return null;

  if (resolveAssistantFailureNotice(message, t)) {
    return buildStatusEntry(t('messenger.messageStatus.error'), 'error');
  }

  if (hasPendingQuestionPanel(message)) {
    return buildStatusEntry(t('messenger.messageStatus.waitingInput'), 'warning', true);
  }

  const workflowItems = Array.isArray(message?.workflowItems)
    ? (message.workflowItems as WorkflowItemLike[])
    : [];
  const latestRetry = findLastWorkflowItem(
    workflowItems,
    (item) => normalizeWorkflowEventType(item?.eventType ?? item?.event) === 'llm_stream_retry'
  );
  const latestQueue = findLastWorkflowItem(
    workflowItems,
    (item) => {
      const eventType = normalizeWorkflowEventType(item?.eventType ?? item?.event);
      return eventType === 'queued' || eventType === 'queue_enter' || eventType === 'queue_start';
    }
  );
  const latestRequest = findLastWorkflowItem(
    workflowItems,
    (item) => normalizeWorkflowEventType(item?.eventType ?? item?.event) === 'llm_request'
  );
  const latestOutput = findLastWorkflowItem(
    workflowItems,
    (item) => {
      const eventType = normalizeWorkflowEventType(item?.eventType ?? item?.event);
      return eventType === 'llm_output' || eventType === 'llm_output_delta';
    }
  );
  const latestActiveTool = findLastWorkflowItem(
    workflowItems,
    (item) => {
      const status = normalizeWorkflowStatus(item?.status);
      if (!ACTIVE_WORKFLOW_STATUSES.has(status)) return false;
      return isToolWorkflowItem(item);
    }
  );
  const latestActiveModelOutput = findLastWorkflowItem(
    workflowItems,
    (item) => {
      const status = normalizeWorkflowStatus(item?.status);
      if (!ACTIVE_WORKFLOW_STATUSES.has(status)) return false;
      return isModelOutputWorkflowItem(item);
    }
  );
  if (message?.resume_available && !isAssistantStreaming(message)) {
    return buildStatusEntry(t('messenger.messageStatus.resumable'), 'warning');
  }
  if (latestRetry.index >= 0 && latestRetry.index >= latestOutput.index) {
    return buildStatusEntry(t('messenger.messageStatus.retrying'), 'warning', true);
  }
  if (message?.slow_client && !hasAssistantVisibleOutput(message)) {
    return buildStatusEntry(t('messenger.messageStatus.retrying'), 'warning', true);
  }
  if (latestQueue.index >= 0 && latestQueue.index >= latestRequest.index && latestQueue.index >= latestOutput.index) {
    return buildStatusEntry(t('messenger.messageStatus.queued'), 'muted', true);
  }
  if (isCompactionRunningFromWorkflowItems(message?.workflowItems)) {
    return buildStatusEntry(t('messenger.messageStatus.compacting'), 'warning', true);
  }
  if (latestActiveTool.index >= 0 && latestActiveTool.index >= latestOutput.index) {
    return buildStatusEntry(t('messenger.messageStatus.toolRunning'), 'running', true);
  }
  if (isAssistantStreaming(message)) {
    if (hasAssistantWaitingForCurrentOutput(message)) {
      return buildStatusEntry(t('messenger.messageStatus.requesting'), 'running', true);
    }
    if (latestActiveModelOutput.index >= 0 || message?.reasoningStreaming) {
      return buildStatusEntry(t('messenger.messageStatus.modelOutputting'), 'running', true);
    }
    if (hasAssistantVisibleOutput(message) || latestOutput.index >= 0) {
      return buildStatusEntry(t('messenger.messageStatus.modelOutputting'), 'running', true);
    }
    if (latestRequest.index >= 0) {
      return buildStatusEntry(t('messenger.messageStatus.requesting'), 'running', true);
    }
    return buildStatusEntry(t('messenger.messageStatus.running'), 'running', true);
  }

  return buildStatusEntry(t('messenger.messageStatus.done'), 'success');
};

export const buildAssistantMessageStatsEntries = (
  message: Record<string, any> | null | undefined,
  t: TranslateFn
): MessageStatsEntry[] => {
  if (!message || message.role !== 'assistant' || message.isGreeting) {
    return [];
  }
  const statusEntry = resolveAssistantStatusEntry(message, t);
  const stats = (message.stats || null) as Record<string, any> | null;
  if (!stats) return statusEntry ? [statusEntry] : [];
  if (isAssistantStreaming(message)) {
    return statusEntry ? [statusEntry] : [];
  }
  const durationSeconds = resolveDurationSeconds(stats);
  const speed = resolveTokenSpeed(stats);
  const usageInputTokens = Number(
    stats?.usage?.input ?? stats?.usage?.input_tokens ?? stats?.usage?.inputTokens
  );
  const usageTotalTokens = Number(
    stats?.usage?.total ?? stats?.usage?.total_tokens ?? stats?.usage?.totalTokens
  );
  const roundUsageInputTokens = Number(
    stats?.roundUsage?.input ??
      stats?.roundUsage?.input_tokens ??
      stats?.roundUsage?.inputTokens ??
      stats?.round_usage?.input ??
      stats?.round_usage?.input_tokens ??
      stats?.round_usage?.inputTokens
  );
  const roundUsageTotalTokens = Number(
    stats?.roundUsage?.total ??
      stats?.roundUsage?.total_tokens ??
      stats?.roundUsage?.totalTokens ??
      stats?.round_usage?.total ??
      stats?.round_usage?.total_tokens ??
      stats?.round_usage?.totalTokens
  );
  const explicitContextTokens = Number(
    stats?.contextTokens ??
      stats?.contextOccupancyTokens ??
      stats?.context_occupancy_tokens ??
      stats?.context_tokens ??
      stats?.context_tokens_total ??
      stats?.context_usage?.context_tokens ??
      stats?.context_usage?.contextTokens
  );
  const contextTokens =
    (Number.isFinite(roundUsageTotalTokens) && roundUsageTotalTokens > 0
      ? roundUsageTotalTokens
      : null) ??
    (Number.isFinite(roundUsageInputTokens) && roundUsageInputTokens > 0
      ? roundUsageInputTokens
      : null) ??
    (Number.isFinite(usageTotalTokens) && usageTotalTokens > 0
      ? usageTotalTokens
      : null) ??
    (Number.isFinite(usageInputTokens) && usageInputTokens > 0 ? usageInputTokens : null) ??
    (Number.isFinite(explicitContextTokens) && explicitContextTokens > 0
      ? explicitContextTokens
      : null) ??
    null;
  const hasUsage = Number.isFinite(Number(contextTokens)) && Number(contextTokens) > 0;
  const hasDuration = Number.isFinite(Number(durationSeconds)) && Number(durationSeconds) > 0;
  const hasSpeed = Number.isFinite(Number(speed)) && Number(speed) > 0;
  const hasToolCalls = Number.isFinite(Number(stats?.toolCalls)) && Number(stats.toolCalls) > 0;
  if (!hasUsage && !hasDuration && !hasToolCalls && !hasSpeed) {
    return statusEntry ? [statusEntry] : [];
  }
  const entries: MessageStatsEntry[] = [];
  if (statusEntry) {
    entries.push(statusEntry);
  }
  entries.push(
    { key: 'duration', label: t('chat.stats.duration'), value: formatDuration(durationSeconds), kind: 'metric' },
    { key: 'speed', label: t('chat.stats.speed'), value: formatSpeed(speed), kind: 'metric' },
    {
      key: 'contextTokens',
      label: t('chat.stats.contextTokens'),
      value: formatCount(contextTokens),
      kind: 'metric'
    },
    { key: 'toolCalls', label: t('chat.stats.toolCalls'), value: formatCount(stats?.toolCalls), kind: 'metric' }
  );
  return entries;
};
