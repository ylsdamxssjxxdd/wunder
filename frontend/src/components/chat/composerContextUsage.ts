export type ComposerContextStatsSource = Record<string, unknown> | null | undefined;

export type ComposerContextSessionSource = Record<string, unknown> | null | undefined;

export type ComposerContextUsageSource = {
  contextTokens: number | null;
  contextTotalTokens: number | null;
  assistantSignature: string;
  runningAssistant: boolean;
  runningContextTokens: number | null;
  contextResetSignature: string;
};

export type ComposerRunningContextDisplayInput = {
  stableTokens: number | null;
  baseTokens: number | null;
  rawBaseTokens: number | null;
  lastRawTokens: number | null;
  runningRawTokens: number;
};

export type ComposerRunningContextDisplayState = {
  stableTokens: number | null;
  baseTokens: number | null;
  rawBaseTokens: number | null;
  lastRawTokens: number | null;
};

const normalizeTokenCount = (value: unknown): number | null => {
  if (value === null || value === undefined) {
    return null;
  }
  const normalizedValue = typeof value === 'string' ? value.trim() : value;
  if (normalizedValue === '') {
    return null;
  }
  const parsed = Number(normalizedValue);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return null;
  }
  return Math.round(parsed);
};

const normalizePositiveTokenCount = (value: unknown): number | null => {
  const normalized = normalizeTokenCount(value);
  if (normalized === null || normalized <= 0) {
    return null;
  }
  return normalized;
};

const resolveContextUsageRecord = (source: Record<string, unknown>): Record<string, unknown> | null => {
  const value = source.context_usage ?? source.contextUsage;
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
};

type ComposerCompactionContextReset = {
  signature: string;
  afterTokens: number | null;
};

const asObjectRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const normalizeText = (value: unknown): string => String(value ?? '').trim().toLowerCase();

const parseDetailRecord = (value: unknown): Record<string, unknown> | null => {
  const direct = asObjectRecord(value);
  if (direct) return direct;
  const text = String(value ?? '').trim();
  if (!text) return null;
  try {
    return asObjectRecord(JSON.parse(text));
  } catch {
    return null;
  }
};

const isComposerCompactionWorkflowItem = (value: unknown): boolean => {
  const record = asObjectRecord(value);
  if (!record) return false;
  const eventType = normalizeText(record.eventType || record.event);
  const toolName = normalizeText(record.toolName || record.tool || record.name);
  const toolCallId = normalizeText(record.toolCallId || record.tool_call_id);
  if (
    eventType === 'compaction' ||
    eventType === 'compaction_progress' ||
    eventType === 'compaction_notice'
  ) {
    return true;
  }
  if (
    toolName === 'context_compaction' ||
    toolName === 'context_compact' ||
    toolName === 'compaction'
  ) {
    return true;
  }
  return toolCallId.startsWith('compaction:');
};

const isCompletedCompactionResetItem = (
  item: Record<string, unknown>,
  detail: Record<string, unknown> | null
): boolean => {
  const detailStatus = normalizeText(detail?.status);
  const itemStatus = normalizeText(item.status);
  const result = normalizeText(detail?.result ?? detail?.compaction_result);
  const negative = new Set([
    'failed',
    'error',
    'cancelled',
    'canceled',
    'aborted',
    'skip',
    'skipped',
    'not_needed',
    'not-needed',
    'unchanged'
  ]);
  if (negative.has(detailStatus) || negative.has(itemStatus) || negative.has(result)) {
    return false;
  }
  const positive = new Set(['done', 'completed', 'complete', 'success', 'succeeded', 'ok']);
  if (positive.has(detailStatus) || positive.has(itemStatus) || positive.has(result)) {
    return true;
  }
  return itemStatus === 'completed';
};

const firstTokenCount = (...values: unknown[]): number | null => {
  for (const value of values) {
    const normalized = normalizeTokenCount(value);
    if (normalized !== null) return normalized;
  }
  return null;
};

const resolveCompactionAfterTokens = (detail: Record<string, unknown> | null): number | null => {
  if (!detail) return null;
  return firstTokenCount(
    detail.final_context_tokens,
    detail.observed_context_tokens_after,
    detail.context_tokens_after,
    detail.context_guard_tokens_after
  );
};

const resolveMessageCompletedCompactionReset = (
  message: Record<string, unknown>,
  messageIndex: number
): ComposerCompactionContextReset | null => {
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  for (let cursor = items.length - 1; cursor >= 0; cursor -= 1) {
    const item = asObjectRecord(items[cursor]);
    if (!item || !isComposerCompactionWorkflowItem(item)) continue;
    const eventType = normalizeText(item.eventType || item.event);
    if (eventType === 'compaction_notice') continue;
    const detail = parseDetailRecord(item.detail ?? item.data ?? item.payload);
    if (!isCompletedCompactionResetItem(item, detail)) continue;
    const afterTokens = resolveCompactionAfterTokens(detail);
    const workflowRef = normalizeText(
      item.toolCallId ??
        item.tool_call_id ??
        item.callId ??
        item.call_id ??
        detail?.compaction_id ??
        detail?.compactionId
    );
    const status = normalizeText(detail?.status ?? item.status);
    const source = normalizeText(
      detail?.context_usage_source_after ??
        detail?.contextUsageSourceAfter ??
        detail?.context_usage_source ??
        detail?.contextUsageSource
    );
    return {
      afterTokens,
      signature: [messageIndex, cursor, workflowRef, status, source, afterTokens ?? '']
        .map((part) => String(part))
        .join(':')
    };
  }
  return null;
};

const resolveCompactionContextTokens = (
  reset: ComposerCompactionContextReset | null,
  sessionTokens: number | null,
  assistantTokens: number | null
): number | null => {
  if (!reset) return null;
  if (
    sessionTokens !== null &&
    (assistantTokens === null ||
      sessionTokens <= assistantTokens ||
      reset.afterTokens === null ||
      sessionTokens <= reset.afterTokens)
  ) {
    return sessionTokens;
  }
  if (
    reset.afterTokens !== null &&
    (assistantTokens === null || reset.afterTokens <= assistantTokens)
  ) {
    return reset.afterTokens;
  }
  return null;
};

const buildContextResetSignature = (
  reset: ComposerCompactionContextReset | null,
  contextTokens: number | null
): string => {
  if (!reset) return '';
  return [reset.signature, contextTokens ?? reset.afterTokens ?? ''].join('@');
};

const resolveExplicitAssistantContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  const contextUsage = resolveContextUsageRecord(stats);
  return normalizePositiveTokenCount(
    stats.context_occupancy_tokens ??
      stats.contextOccupancyTokens ??
      contextUsage?.context_occupancy_tokens ??
      contextUsage?.contextOccupancyTokens ??
      stats.contextTokens ??
      stats.context_tokens ??
      contextUsage?.contextTokens ??
      contextUsage?.context_tokens
  );
};

const resolveAssistantContextPreviewTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  return normalizePositiveTokenCount(
    stats.contextPreviewTokens ??
      stats.context_preview_tokens ??
      stats.contextEstimateTokens ??
      stats.context_estimate_tokens ??
      stats.estimatedContextTokens ??
      stats.estimated_context_tokens
  );
};

const resolveFinalAssistantContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  return resolveExplicitAssistantContextTokens(stats);
};

export const resolveComposerRunningContextDisplayState = (
  input: ComposerRunningContextDisplayInput
): ComposerRunningContextDisplayState => {
  let baseTokens = input.baseTokens;
  let rawBaseTokens = input.rawBaseTokens;
  const current = input.stableTokens;
  const runningRaw = input.runningRawTokens;
  if (baseTokens === null && current !== null) {
    baseTokens = current;
  }
  if (rawBaseTokens === null) {
    rawBaseTokens = runningRaw;
  }
  if (input.lastRawTokens !== null && runningRaw < input.lastRawTokens && current !== null) {
    baseTokens = current;
    rawBaseTokens = runningRaw;
  }
  if (baseTokens === null) {
    return {
      stableTokens: current === null ? runningRaw : Math.max(current, runningRaw),
      baseTokens,
      rawBaseTokens,
      lastRawTokens: runningRaw
    };
  }
  const displayTokens =
    runningRaw >= baseTokens
      ? runningRaw
      : baseTokens + Math.max(0, runningRaw - rawBaseTokens);
  return {
    stableTokens: current === null ? displayTokens : Math.max(current, displayTokens),
    baseTokens,
    rawBaseTokens,
    lastRawTokens: runningRaw
  };
};

export const formatContextTokenCount = (value: unknown): string => {
  const normalized = normalizeTokenCount(value);
  if (normalized === null) return '--';
  return String(normalized);
};

export const resolveStableComposerContextPair = (
  used: number | null,
  total: number | null
): { used: number | null; total: number | null } => {
  if (used === null && total === null) {
    return { used: null, total: null };
  }
  if (total === null || total <= 0) {
    return { used: null, total: null };
  }
  return { used, total };
};

export const resolveAssistantContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  return resolveFinalAssistantContextTokens(stats);
};

const resolveAssistantLiveContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  return resolveExplicitAssistantContextTokens(stats);
};

export const resolveAssistantContextTotalTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  const contextUsage = resolveContextUsageRecord(stats);
  return normalizePositiveTokenCount(
    stats.contextTotalTokens ??
      stats.context_total_tokens ??
      stats.context_max_tokens ??
      stats.max_context ??
      stats.maxContext ??
      stats.context_window ??
      contextUsage?.max_context ??
      contextUsage?.context_max_tokens
  );
};

export const resolveSessionContextTokens = (session: ComposerContextSessionSource): number | null => {
  if (!session) {
    return null;
  }
  const contextUsage = resolveContextUsageRecord(session);
  return normalizePositiveTokenCount(
    session.context_occupancy_tokens ??
      session.contextOccupancyTokens ??
      contextUsage?.context_occupancy_tokens ??
      contextUsage?.contextOccupancyTokens ??
      session.contextTokens ??
      session.context_tokens ??
      contextUsage?.contextTokens ??
      contextUsage?.context_tokens
  );
};

const resolveSessionContextTokensIncludingZero = (
  session: ComposerContextSessionSource
): number | null => {
  if (!session) {
    return null;
  }
  const contextUsage = resolveContextUsageRecord(session);
  return normalizeTokenCount(
    session.context_occupancy_tokens ??
      session.contextOccupancyTokens ??
      contextUsage?.context_occupancy_tokens ??
      contextUsage?.contextOccupancyTokens ??
      session.contextTokens ??
      session.context_tokens ??
      contextUsage?.contextTokens ??
      contextUsage?.context_tokens
  );
};

export const resolveSessionContextTotalTokens = (session: ComposerContextSessionSource): number | null => {
  if (!session) {
    return null;
  }
  const contextUsage = resolveContextUsageRecord(session);
  return normalizePositiveTokenCount(
    session.contextTotalTokens ??
      session.context_total_tokens ??
      session.context_max_tokens ??
      session.max_context ??
      session.maxContext ??
      session.context_window ??
      contextUsage?.max_context ??
      contextUsage?.context_max_tokens
  );
};

const isAssistantMessageRunning = (message: Record<string, unknown>): boolean =>
  Boolean(
    message.stream_incomplete ||
      message.workflowStreaming ||
      message.reasoningStreaming ||
      message.waiting_for_output ||
      message.waitingForOutput
  );

const hasMessageContent = (value: unknown): boolean => String(value || '').trim().length > 0;

const hasPlanSteps = (plan: unknown): boolean =>
  Array.isArray((plan as { steps?: unknown[] } | null)?.steps) &&
  (((plan as { steps?: unknown[] } | null)?.steps?.length) || 0) > 0;

const isCompactionOnlyWorkflowItems = (items: unknown): boolean => {
  if (!Array.isArray(items) || items.length === 0) return false;
  let hasCompaction = false;
  for (const item of items) {
    if (!isComposerCompactionWorkflowItem(item)) {
      return false;
    }
    hasCompaction = true;
  }
  return hasCompaction;
};

const isCompactionMarkerAssistantMessage = (message: Record<string, unknown>): boolean => {
  if (String(message.role || '').trim().toLowerCase() !== 'assistant') return false;
  if (hasMessageContent(message.content) || hasMessageContent(message.reasoning)) return false;
  if (hasPlanSteps(message.plan)) return false;
  const panelStatus = String(
    ((message.questionPanel as Record<string, unknown> | null)?.status || '')
  )
    .trim()
    .toLowerCase();
  if (panelStatus === 'pending') return false;
  if (message.manual_compaction_marker === true || message.manualCompactionMarker === true) {
    return true;
  }
  if (!isCompactionOnlyWorkflowItems(message.workflowItems)) return false;
  if (!isAssistantMessageRunning(message)) return true;
  const workflowItems = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  return workflowItems.some((item) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)) return false;
    const record = item as Record<string, unknown>;
    const detailRaw = record.detail;
    if (typeof detailRaw !== 'string') return false;
    try {
      const detail = JSON.parse(detailRaw) as Record<string, unknown>;
      return String(detail?.trigger_mode ?? detail?.triggerMode ?? '').trim().toLowerCase() === 'manual';
    } catch {
      return false;
    }
  });
};

const isGoalMarkerAssistantMessage = (message: Record<string, unknown>): boolean =>
  String(message.role || '').trim().toLowerCase() === 'assistant' &&
  hasMessageContent(message.content) &&
  (message.manual_goal_marker === true || message.manualGoalMarker === true);

const shouldSkipComposerContextAssistant = (message: Record<string, unknown>): boolean =>
  Boolean(
    message.isGreeting ||
      isCompactionMarkerAssistantMessage(message) ||
      isGoalMarkerAssistantMessage(message)
  );

export const resolveComposerContextUsageSource = (
  messages: unknown[],
  session: ComposerContextSessionSource,
  loading: boolean
): ComposerContextUsageSource => {
  const list = Array.isArray(messages) ? messages : [];
  let trailingCompactionReset: ComposerCompactionContextReset | null = null;
  for (let cursor = list.length - 1; cursor >= 0; cursor -= 1) {
    const current =
      list[cursor] && typeof list[cursor] === 'object'
        ? (list[cursor] as Record<string, unknown>)
        : null;
    if (!current) continue;
    if (String(current.role || '').trim().toLowerCase() !== 'assistant') continue;
    const ownCompactionReset = resolveMessageCompletedCompactionReset(current, cursor);
    if (shouldSkipComposerContextAssistant(current)) {
      if (ownCompactionReset) {
        trailingCompactionReset = ownCompactionReset;
      }
      continue;
    }
    const stats =
      current.stats && typeof current.stats === 'object'
        ? (current.stats as Record<string, unknown>)
        : null;
    const runningAssistant = loading && isAssistantMessageRunning(current);
    const assistantContextTokens = runningAssistant
      ? resolveAssistantLiveContextTokens(stats)
      : resolveAssistantContextTokens(stats);
    const assistantPreviewTokens =
      assistantContextTokens === null ? resolveAssistantContextPreviewTokens(stats) : null;
    const assistantTotalTokens = resolveAssistantContextTotalTokens(stats);
    const sessionContextTokens = resolveSessionContextTokens(session);
    const sessionContextTokensAfterCompaction = resolveSessionContextTokensIncludingZero(session);
    const sessionTotalTokens = resolveSessionContextTotalTokens(session);
    const compactionReset = ownCompactionReset ?? trailingCompactionReset;
    const compactionContextTokens = resolveCompactionContextTokens(
      compactionReset,
      sessionContextTokensAfterCompaction,
      assistantContextTokens
    );
    const contextResetSignature = buildContextResetSignature(
      compactionReset,
      compactionContextTokens
    );
    const fallbackContextTokens =
      compactionContextTokens ?? sessionContextTokens ?? assistantPreviewTokens;
    const source = {
      contextTokens: compactionContextTokens ?? assistantContextTokens,
      contextTotalTokens: assistantTotalTokens,
      assistantSignature: [
        cursor,
        String(current.created_at ?? current.createdAt ?? '')
      ].join(':'),
      runningAssistant,
      runningContextTokens: runningAssistant
        ? compactionContextTokens ?? assistantContextTokens
        : null,
      contextResetSignature
    };
    if (runningAssistant) {
      return {
        ...source,
        contextTokens: compactionContextTokens ?? assistantContextTokens ?? fallbackContextTokens,
        contextTotalTokens:
          assistantTotalTokens !== null && sessionTotalTokens !== null
            ? Math.max(assistantTotalTokens, sessionTotalTokens)
            : assistantTotalTokens ?? sessionTotalTokens
      };
    }
    return {
      ...source,
      contextTokens:
        compactionContextTokens ??
        (assistantContextTokens !== null && sessionContextTokens !== null
          ? Math.max(assistantContextTokens, sessionContextTokens)
          : assistantContextTokens ?? fallbackContextTokens),
      contextTotalTokens:
        assistantTotalTokens !== null && sessionTotalTokens !== null
          ? Math.max(assistantTotalTokens, sessionTotalTokens)
          : assistantTotalTokens ?? sessionTotalTokens
    };
  }
  const sessionContextTokens = resolveSessionContextTokens(session);
  const sessionContextTokensAfterCompaction = resolveSessionContextTokensIncludingZero(session);
  const compactionContextTokens = resolveCompactionContextTokens(
    trailingCompactionReset,
    sessionContextTokensAfterCompaction,
    null
  );
  return {
    contextTokens: compactionContextTokens ?? sessionContextTokens,
    contextTotalTokens: resolveSessionContextTotalTokens(session),
    assistantSignature: '',
    runningAssistant: false,
    runningContextTokens: null,
    contextResetSignature: buildContextResetSignature(
      trailingCompactionReset,
      compactionContextTokens
    )
  };
};
