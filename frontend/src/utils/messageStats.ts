import { normalizeChatDurationSeconds } from './chatTiming';
import { resolveAssistantFailureNotice } from './assistantFailureNotice';
import {
  hasAssistantPendingQuestion,
  hasAssistantWaitingForCurrentOutput,
  isAssistantMessageRunning
} from './assistantMessageRuntime';
import { isCompactionRunningFromWorkflowItems } from './chatCompactionWorkflow';
import { shouldDisplayTransientRetry } from './retryVisibility';
import { hasActiveSubagentItems } from './subagentRuntime';

export type MessageStatsEntry = {
  key: string;
  label: string;
  value: string;
  kind?: 'status' | 'metric';
  tone?: 'running' | 'warning' | 'success' | 'error' | 'muted';
  live?: boolean;
  iconClass?: string;
};

type TranslateFn = (key: string, params?: Record<string, unknown>) => string;
type WorkflowItemLike = Record<string, any>;
type MessageLike = Record<string, any>;

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

const formatCompactElapsed = (milliseconds: number): string => {
  if (!Number.isFinite(milliseconds) || milliseconds <= 0) return '';
  const totalSeconds = Math.max(1, Math.round(milliseconds / 1000));
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) {
    return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
  }
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes > 0 ? `${hours}h ${restMinutes}m` : `${hours}h`;
};

const resolveQueueAheadCount = (item: WorkflowItemLike | null | undefined): number | null => {
  if (!item) return null;
  const detail =
    typeof item.detail === 'string'
      ? (() => {
          try {
            return JSON.parse(item.detail);
          } catch {
            return null;
          }
        })()
      : item.detail && typeof item.detail === 'object'
        ? item.detail
        : null;
  const candidates = [
    item.queue_ahead,
    item.queueAhead,
    detail?.queue_ahead,
    detail?.queueAhead,
    detail?.data?.queue_ahead,
    detail?.data?.queueAhead
  ];
  for (const candidate of candidates) {
    const parsed = Number.parseInt(String(candidate ?? ''), 10);
    if (Number.isFinite(parsed) && parsed >= 0) {
      return parsed;
    }
  }
  return null;
};

const normalizeSpeed = (speed: number): number | null => {
  if (!Number.isFinite(speed) || speed <= 0) return null;
  return speed;
};

const parsePositiveInteger = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const isMeaningfulConsumedTokens = (value: number | null): value is number =>
  value !== null && Number.isFinite(value) && value > 1;

const resolveUsageConsumedTokens = (value: unknown): number | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const record = value as Record<string, any>;
  return (
    parsePositiveInteger(record.total ?? record.total_tokens ?? record.totalTokens) ??
    parsePositiveInteger(record.input ?? record.input_tokens ?? record.inputTokens)
  );
};

const resolveQuotaConsumedValue = (value: unknown): number | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return parsePositiveInteger(value);
  }
  const record = value as Record<string, any>;
  return (
    parsePositiveInteger(
      record.request_consumed_tokens ??
        record.requestConsumedTokens ??
        record.consumed_tokens ??
        record.consumedTokens ??
        record.consumed ??
        record.used ??
        record.count
    ) ?? null
  );
};

const resolveExplicitConsumedTokens = (source: Record<string, any> | null | undefined): number | null => {
  if (!source || typeof source !== 'object') return null;
  return (
    parsePositiveInteger(
      source.request_consumed_tokens ??
        source.requestConsumedTokens ??
        source.consumed_tokens ??
        source.consumedTokens
    ) ??
    resolveQuotaConsumedValue(source.quotaConsumed ?? source.quota_consumed ?? source.quota)
  );
};

const resolveRoundConsumedTokens = (source: Record<string, any> | null | undefined): number | null => {
  if (!source || typeof source !== 'object') return null;
  const roundUsage =
    source.roundUsage ??
    source.round_usage ??
    source.round_usage_total ??
    source.billedUsage ??
    source.billed_usage;
  return (
    resolveQuotaConsumedValue(roundUsage) ??
    resolveUsageConsumedTokens(roundUsage) ??
    parsePositiveInteger(roundUsage)
  );
};

const resolvePartialConsumedTokens = (source: Record<string, any> | null | undefined): number | null => {
  if (!source || typeof source !== 'object') return null;
  return parsePositiveInteger(
    source.partialQuotaConsumed ??
      source.partial_quota_consumed ??
      source.partialConsumedTokens ??
      source.partial_consumed_tokens
  );
};

const resolveExplicitContextTokens = (stats: Record<string, any> | null | undefined): number | null => {
  if (!stats || typeof stats !== 'object') return null;
  return parsePositiveInteger(
    stats.contextTokens ??
      stats.contextOccupancyTokens ??
      stats.context_occupancy_tokens ??
      stats.context_tokens ??
      stats.context_tokens_total ??
      stats.context_usage?.context_tokens ??
      stats.context_usage?.contextTokens
  );
};

const resolveContextTokens = (stats: Record<string, any> | null | undefined): number | null => {
  if (!stats || typeof stats !== 'object') return null;
  return (
    resolveUsageConsumedTokens(stats.usage) ??
    resolveUsageConsumedTokens(stats.roundUsage ?? stats.round_usage) ??
    resolveExplicitContextTokens(stats)
  );
};

const resolveAssistantTurnConsumedTokens = (
  message: MessageLike,
  allMessages?: MessageLike[] | null
): number | null => {
  if (!Array.isArray(allMessages) || allMessages.length === 0) return null;
  const currentIndex = resolveMessageIndex(message, allMessages);
  if (currentIndex < 0) return null;
  let start = currentIndex;
  while (start > 0) {
    const previous = allMessages[start - 1];
    if (previous?.role === 'user') break;
    start -= 1;
  }
  let end = currentIndex + 1;
  while (end < allMessages.length) {
    const next = allMessages[end];
    if (next?.role === 'user') break;
    end += 1;
  }
  let total = 0;
  let found = false;
  for (let index = start; index < end; index += 1) {
    const candidate = allMessages[index];
    if (!candidate || candidate.role !== 'assistant' || candidate.isGreeting) continue;
    const consumed =
      resolveExplicitConsumedTokens(candidate?.stats as Record<string, any> | null | undefined) ??
      resolveExplicitConsumedTokens(candidate);
    if (!isMeaningfulConsumedTokens(consumed)) continue;
    total += consumed;
    found = true;
  }
  return found ? total : null;
};

const resolveAssistantConsumedTokens = (
  message: MessageLike,
  allMessages?: MessageLike[] | null
): number | null => {
  const stats =
    message?.stats && typeof message.stats === 'object'
      ? (message.stats as Record<string, any>)
      : null;
  const aggregatedTurnConsumedTokens = resolveAssistantTurnConsumedTokens(message, allMessages);
  const directConsumedTokens =
    resolveExplicitConsumedTokens(stats) ?? resolveExplicitConsumedTokens(message);
  const fallbackConsumedTokens =
    resolveRoundConsumedTokens(stats) ??
    resolveRoundConsumedTokens(message) ??
    resolvePartialConsumedTokens(stats) ??
    resolvePartialConsumedTokens(message);
  const explicitConsumedTokens = aggregatedTurnConsumedTokens ?? directConsumedTokens;
  if (isMeaningfulConsumedTokens(explicitConsumedTokens)) {
    return explicitConsumedTokens;
  }
  if (isMeaningfulConsumedTokens(fallbackConsumedTokens)) {
    return fallbackConsumedTokens;
  }
  return null;
};

export const sumConversationConsumedTokens = (messages: MessageLike[] | null | undefined): number => {
  if (!Array.isArray(messages) || messages.length === 0) {
    return 0;
  }
  let userTurnIndex = 0;
  const turnConsumed = new Map<number, number>();
  messages.forEach((message) => {
    if (!message || message.isGreeting) {
      return;
    }
    if (message.role === 'user') {
      userTurnIndex += 1;
      return;
    }
    if (message.role !== 'assistant') {
      return;
    }
    const consumed = resolveAssistantConsumedTokens(message, messages);
    if (consumed === null || consumed <= 0) {
      return;
    }
    const previous = turnConsumed.get(userTurnIndex) ?? 0;
    if (consumed > previous) {
      turnConsumed.set(userTurnIndex, consumed);
    }
  });
  let total = 0;
  turnConsumed.forEach((value) => {
    total += value;
  });
  return total;
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

const hasAssistantVisibleOutput = (message: Record<string, any>): boolean =>
  Boolean(String(message?.content || '').trim()) || Boolean(String(message?.reasoning || '').trim());

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

const buildRetryStatusValue = (
  message: Record<string, any>,
  retryItem: WorkflowItemLike | null,
  t: TranslateFn,
  nowMs = Date.now()
): string => {
  const parts = [t('messenger.messageStatus.retrying')];
  const attempt = parsePositiveInteger(retryItem?.attempt ?? message?.retry_attempt ?? message?.retryAttempt);
  const maxAttempts = parsePositiveInteger(
    retryItem?.maxAttempts ?? message?.retry_max_attempts ?? message?.retryMaxAttempts
  );
  if (attempt && maxAttempts) {
    parts.push(t('messenger.messageStatus.retryAttemptCompact', { attempt, maxAttempts }));
  } else if (attempt) {
    parts.push(t('messenger.messageStatus.retryAttemptCompactSingle', { attempt }));
  }
  const retryNextAttemptAtMs = Number(
    message?.retry_next_attempt_at_ms ?? message?.retryNextAttemptAtMs
  );
  const retryDelaySeconds = Number(
    retryItem?.delayS ?? message?.retry_delay_s ?? message?.retryDelayS
  );
  const retryDelayMs =
    Number.isFinite(retryNextAttemptAtMs) && retryNextAttemptAtMs > nowMs
      ? retryNextAttemptAtMs - nowMs
      : Number.isFinite(retryDelaySeconds) && retryDelaySeconds > 0
        ? retryDelaySeconds * 1000
        : 0;
  const retryDelayLabel = retryDelayMs > 0 ? formatCompactElapsed(retryDelayMs) : '';
  if (retryDelayLabel) {
    parts.push(t('messenger.messageStatus.retryDelayCompact', { delay: retryDelayLabel }));
    return parts.join(' · ');
  }
  const retryStartedAtMs = Number(
    message?.retry_started_at_ms ??
      message?.retryStartedAtMs ??
      message?.waiting_updated_at_ms ??
      message?.waitingUpdatedAtMs ??
      message?.stats?.interaction_start_ms
  );
  if (Number.isFinite(retryStartedAtMs) && retryStartedAtMs > 0) {
    const elapsedLabel = formatCompactElapsed(nowMs - retryStartedAtMs);
    if (elapsedLabel) {
      parts.push(t('messenger.messageStatus.retryElapsedCompact', { time: elapsedLabel }));
    }
  }
  return parts.join(' · ');
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

const hasAssistantActivitySignals = (message: Record<string, any> | null | undefined): boolean => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return false;
  return Boolean(
    hasAssistantPendingQuestion(message) ||
      isAssistantMessageRunning(message) ||
      message?.resume_available ||
      message?.slow_client ||
      parsePositiveInteger(message?.retry_attempt ?? message?.retryAttempt) ||
      parsePositiveInteger(message?.retry_max_attempts ?? message?.retryMaxAttempts) ||
      Number.isFinite(Number(message?.retry_started_at_ms ?? message?.retryStartedAtMs)) ||
      Number.isFinite(Number(message?.retry_next_attempt_at_ms ?? message?.retryNextAttemptAtMs)) ||
      normalizeWorkflowStatus(message?.retry_state ?? message?.retryState) === 'retrying' ||
      (Array.isArray(message?.workflowItems) && message.workflowItems.length > 0) ||
      hasActiveSubagentItems(message?.subagents) ||
      (Array.isArray(message?.subagents) && message.subagents.length > 0) ||
      (message?.stats && typeof message.stats === 'object')
  );
};

const isAssistantCompactionMessageRunning = (message: MessageLike | null | undefined): boolean => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return false;
  return isCompactionRunningFromWorkflowItems(message?.workflowItems);
};

const resolveMessageIndex = (message: MessageLike, allMessages: MessageLike[]): number => {
  const directIndex = allMessages.lastIndexOf(message);
  if (directIndex >= 0) return directIndex;
  const role = String(message?.role || '');
  const createdAt = String(message?.created_at || '');
  const streamEventId = String(message?.stream_event_id || '');
  const streamRound = String(message?.stream_round || '');
  for (let index = allMessages.length - 1; index >= 0; index -= 1) {
    const item = allMessages[index];
    if (!item || typeof item !== 'object') continue;
    if (
      String(item?.role || '') === role &&
      String(item?.created_at || '') === createdAt &&
      String(item?.stream_event_id || '') === streamEventId &&
      String(item?.stream_round || '') === streamRound
    ) {
      return index;
    }
  }
  return -1;
};

const hasConversationCompactionRunning = (
  message: MessageLike,
  allMessages: MessageLike[] | null | undefined
): boolean => {
  if (!Array.isArray(allMessages) || allMessages.length < 2) return false;
  const currentIndex = resolveMessageIndex(message, allMessages);
  if (currentIndex < 0) return false;
  return allMessages.some((item, index) => {
    if (index === currentIndex || index < currentIndex) return false;
    return isAssistantCompactionMessageRunning(item);
  });
};

const buildStatusEntry = (
  value: string,
  tone: NonNullable<MessageStatsEntry['tone']>,
  live = false,
  iconClass = 'fa-solid fa-circle-info'
): MessageStatsEntry => ({
  key: 'status',
  label: '',
  value,
  kind: 'status',
  tone,
  live,
  iconClass
});

const resolveAssistantStatusEntry = (
  message: Record<string, any>,
  t: TranslateFn,
  allMessages?: MessageLike[] | null,
  nowMs = Date.now()
): MessageStatsEntry | null => {
  if (!hasAssistantActivitySignals(message)) return null;

  if (resolveAssistantFailureNotice(message, t)) {
    return buildStatusEntry(t('messenger.messageStatus.error'), 'error', false, 'fa-solid fa-triangle-exclamation');
  }

  if (hasAssistantPendingQuestion(message)) {
    return buildStatusEntry(t('messenger.messageStatus.waitingInput'), 'warning', true, 'fa-solid fa-circle-question');
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
      return (
        eventType === 'queued' ||
        eventType === 'queue_enter' ||
        eventType === 'queue_update' ||
        eventType === 'queue_start'
      );
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
  const hasPersistedRetryState = Boolean(
    parsePositiveInteger(message?.retry_attempt ?? message?.retryAttempt) ||
      parsePositiveInteger(message?.retry_max_attempts ?? message?.retryMaxAttempts) ||
      Number.isFinite(Number(message?.retry_started_at_ms ?? message?.retryStartedAtMs)) ||
      Number.isFinite(Number(message?.retry_next_attempt_at_ms ?? message?.retryNextAttemptAtMs)) ||
      normalizeWorkflowStatus(message?.retry_state ?? message?.retryState) === 'retrying'
  );
  const shouldShowRetryState = shouldDisplayTransientRetry(
    {
      retry_attempt: message?.retry_attempt ?? message?.retryAttempt ?? latestRetry.item?.attempt,
      retry_started_at_ms: message?.retry_started_at_ms ?? message?.retryStartedAtMs
    },
    nowMs
  );
  if (message?.resume_available && !isAssistantMessageRunning(message)) {
    return buildStatusEntry(t('messenger.messageStatus.resumable'), 'warning', false, 'fa-solid fa-rotate-right');
  }
  if (
    shouldShowRetryState &&
    (
      (latestRetry.index >= 0 &&
        latestRetry.index >= latestOutput.index &&
        latestRetry.index >= latestRequest.index)
      || (
        hasPersistedRetryState &&
        latestOutput.index < 0 &&
        latestActiveTool.index < 0 &&
        latestRequest.index < latestRetry.index
      )
    )
  ) {
    return buildStatusEntry(
      buildRetryStatusValue(message, latestRetry.item, t, nowMs),
      'warning',
      true,
      'fa-solid fa-plug-circle-bolt'
    );
  }
  if (message?.slow_client && !hasAssistantVisibleOutput(message)) {
    return buildStatusEntry(
      buildRetryStatusValue(message, latestRetry.item, t, nowMs),
      'warning',
      true,
      'fa-solid fa-plug-circle-bolt'
    );
  }
  if (latestQueue.index >= 0 && latestQueue.index >= latestRequest.index && latestQueue.index >= latestOutput.index) {
    const queueAhead = resolveQueueAheadCount(latestQueue.item);
    const queuedLabel =
      queueAhead !== null
        ? t('messenger.messageStatus.queuedAhead', { count: queueAhead })
        : t('messenger.messageStatus.queued');
    return buildStatusEntry(queuedLabel, 'muted', true, 'fa-solid fa-clock');
  }
  if (isCompactionRunningFromWorkflowItems(message?.workflowItems)) {
    return buildStatusEntry(t('messenger.messageStatus.compacting'), 'warning', true, 'fa-solid fa-compress');
  }
  if (hasActiveSubagentItems(message?.subagents)) {
    return buildStatusEntry(t('messenger.messageStatus.subagentRunning'), 'running', true, 'fa-solid fa-diagram-project');
  }
  if (
    (isAssistantMessageRunning(message) || hasAssistantWaitingForCurrentOutput(message) || latestRequest.index >= 0) &&
    hasConversationCompactionRunning(message, allMessages)
  ) {
    return buildStatusEntry(t('messenger.messageStatus.compacting'), 'warning', true, 'fa-solid fa-compress');
  }
  if (latestActiveTool.index >= 0 && latestActiveTool.index >= latestOutput.index) {
    return buildStatusEntry(t('messenger.messageStatus.toolRunning'), 'running', true, 'fa-solid fa-screwdriver-wrench');
  }
  if (hasAssistantWaitingForCurrentOutput(message)) {
    return buildStatusEntry(t('messenger.messageStatus.requesting'), 'running', true, 'fa-solid fa-paper-plane');
  }
  if (isAssistantMessageRunning(message)) {
    if (latestActiveModelOutput.index >= 0 || message?.reasoningStreaming) {
      return buildStatusEntry(t('messenger.messageStatus.modelOutputting'), 'running', true, 'fa-solid fa-comment-dots');
    }
    if (hasAssistantVisibleOutput(message) || latestOutput.index >= 0) {
      return buildStatusEntry(t('messenger.messageStatus.modelOutputting'), 'running', true, 'fa-solid fa-comment-dots');
    }
    if (latestRequest.index >= 0) {
      return buildStatusEntry(t('messenger.messageStatus.requesting'), 'running', true, 'fa-solid fa-paper-plane');
    }
    // Generic running only drives avatar/composer state; showing it in the bubble causes a visible flash.
    return null;
  }

  return buildStatusEntry(t('messenger.messageStatus.done'), 'success', false, 'fa-solid fa-check');
};

export const buildAssistantMessageStatsEntries = (
  message: Record<string, any> | null | undefined,
  t: TranslateFn,
  allMessages?: MessageLike[] | null,
  nowMs = Date.now()
): MessageStatsEntry[] => {
  if (!message || message.role !== 'assistant' || message.isGreeting) {
    return [];
  }
  const statusEntry = resolveAssistantStatusEntry(message, t, allMessages, nowMs);
  const stats = (message.stats || null) as Record<string, any> | null;
  if (!stats) return statusEntry ? [statusEntry] : [];
  if (
    isAssistantMessageRunning(message) ||
    hasAssistantWaitingForCurrentOutput(message) ||
    hasActiveSubagentItems(message?.subagents)
  ) {
    return statusEntry ? [statusEntry] : [];
  }
  const durationSeconds = resolveDurationSeconds(stats);
  const speed = resolveTokenSpeed(stats);
  const contextTokens = resolveContextTokens(stats);
  const effectiveQuotaConsumedTokens = resolveAssistantConsumedTokens(message, allMessages);
  const hasUsage = Number.isFinite(Number(contextTokens)) && Number(contextTokens) > 0;
  const hasQuota =
    Number.isFinite(Number(effectiveQuotaConsumedTokens)) && Number(effectiveQuotaConsumedTokens) > 0;
  const hasDuration = Number.isFinite(Number(durationSeconds)) && Number(durationSeconds) > 0;
  const hasSpeed = Number.isFinite(Number(speed)) && Number(speed) > 0;
  const hasToolCalls = Number.isFinite(Number(stats?.toolCalls)) && Number(stats.toolCalls) > 0;
  if (!hasUsage && !hasQuota && !hasDuration && !hasToolCalls && !hasSpeed) {
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
    {
      key: 'quota',
      label: t('chat.stats.quota'),
      value: formatCount(effectiveQuotaConsumedTokens),
      kind: 'metric'
    },
    { key: 'toolCalls', label: t('chat.stats.toolCalls'), value: formatCount(stats?.toolCalls), kind: 'metric' }
  );
  return entries;
};
