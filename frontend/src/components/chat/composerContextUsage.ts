export type ComposerContextStatsSource = Record<string, unknown> | null | undefined;

export type ComposerContextSessionSource = Record<string, unknown> | null | undefined;

export type ComposerContextUsageSource = {
  contextTokens: number | null;
  contextTotalTokens: number | null;
  assistantSignature: string;
  runningAssistant: boolean;
  runningContextTokens: number | null;
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

const resolveExplicitAssistantContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  const contextUsage = resolveContextUsageRecord(stats);
  const directContextUsage =
    stats.contextUsage !== null &&
    stats.contextUsage !== undefined &&
    typeof stats.contextUsage !== 'object'
      ? stats.contextUsage
      : null;
  return normalizePositiveTokenCount(
    stats.contextTokens ??
      stats.contextOccupancyTokens ??
      stats.context_occupancy_tokens ??
      stats.context_tokens ??
      stats.context_tokens_total ??
      directContextUsage ??
      contextUsage?.context_occupancy_tokens ??
      contextUsage?.contextOccupancyTokens ??
      contextUsage?.context_tokens ??
      contextUsage?.contextTokens
  );
};

const resolveUsageContextTokens = (usage: unknown): number | null => {
  if (!usage || typeof usage !== 'object' || Array.isArray(usage)) {
    return null;
  }
  const record = usage as Record<string, unknown>;
  return normalizePositiveTokenCount(
    record.total ??
      record.total_tokens ??
      record.totalTokens ??
      record.input ??
      record.input_tokens ??
      record.inputTokens
  );
};

const resolveFinalAssistantContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  return (
    resolveUsageContextTokens(stats.usage) ??
    resolveUsageContextTokens(stats.roundUsage ?? stats.round_usage) ??
    resolveExplicitAssistantContextTokens(stats)
  );
};

const resolveFinalAssistantUsageTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  return (
    resolveUsageContextTokens(stats.usage) ??
    resolveUsageContextTokens(stats.roundUsage ?? stats.round_usage)
  );
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
    session.contextTokens ??
      session.context_tokens ??
      session.contextOccupancyTokens ??
      session.context_occupancy_tokens ??
      contextUsage?.context_tokens ??
      contextUsage?.contextTokens
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

export const resolveComposerContextUsageSource = (
  messages: unknown[],
  session: ComposerContextSessionSource,
  loading: boolean
): ComposerContextUsageSource => {
  const list = Array.isArray(messages) ? messages : [];
  for (let cursor = list.length - 1; cursor >= 0; cursor -= 1) {
    const current =
      list[cursor] && typeof list[cursor] === 'object'
        ? (list[cursor] as Record<string, unknown>)
        : null;
    if (!current) continue;
    if (String(current.role || '').trim().toLowerCase() !== 'assistant') continue;
    const stats =
      current.stats && typeof current.stats === 'object'
        ? (current.stats as Record<string, unknown>)
        : null;
    const runningAssistant = loading && isAssistantMessageRunning(current);
    const assistantFinalUsageTokens = runningAssistant
      ? null
      : resolveFinalAssistantUsageTokens(stats);
    const assistantContextTokens = runningAssistant
      ? resolveAssistantLiveContextTokens(stats)
      : resolveAssistantContextTokens(stats);
    const assistantTotalTokens = resolveAssistantContextTotalTokens(stats);
    const sessionContextTokens = resolveSessionContextTokens(session);
    const sessionTotalTokens = resolveSessionContextTotalTokens(session);
    const source = {
      contextTokens: assistantContextTokens,
      contextTotalTokens: assistantTotalTokens,
      assistantSignature: [
        cursor,
        String(current.created_at ?? current.createdAt ?? '')
      ].join(':'),
      runningAssistant,
      runningContextTokens: runningAssistant ? assistantContextTokens : null
    };
    if (runningAssistant) {
      return {
        ...source,
        contextTokens: assistantContextTokens ?? sessionContextTokens,
        contextTotalTokens:
          assistantTotalTokens !== null && sessionTotalTokens !== null
            ? Math.max(assistantTotalTokens, sessionTotalTokens)
            : assistantTotalTokens ?? sessionTotalTokens
      };
    }
    if (assistantFinalUsageTokens !== null) {
      return {
        ...source,
        contextTokens: assistantFinalUsageTokens,
        contextTotalTokens:
          assistantTotalTokens !== null && sessionTotalTokens !== null
            ? Math.max(assistantTotalTokens, sessionTotalTokens)
            : assistantTotalTokens ?? sessionTotalTokens
      };
    }
    return {
      ...source,
      contextTokens:
        assistantContextTokens !== null && sessionContextTokens !== null
          ? Math.max(assistantContextTokens, sessionContextTokens)
          : assistantContextTokens ?? sessionContextTokens,
      contextTotalTokens:
        assistantTotalTokens !== null && sessionTotalTokens !== null
          ? Math.max(assistantTotalTokens, sessionTotalTokens)
          : assistantTotalTokens ?? sessionTotalTokens
    };
  }
  return {
    contextTokens: resolveSessionContextTokens(session),
    contextTotalTokens: resolveSessionContextTotalTokens(session),
    assistantSignature: '',
    runningAssistant: false,
    runningContextTokens: null
  };
};
