export type ComposerContextStatsSource = Record<string, unknown> | null | undefined;

export type ComposerContextSessionSource = Record<string, unknown> | null | undefined;

export type ComposerContextUsageSource = {
  contextTokens: number | null;
  contextTotalTokens: number | null;
  assistantSignature: string;
  runningAssistant: boolean;
  runningContextTokens: number | null;
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

export const formatContextTokenCount = (value: unknown): string => {
  const normalized = normalizeTokenCount(value);
  if (normalized === null) return '--';
  return String(normalized);
};

export const resolveAssistantContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  const usage = stats.usage as Record<string, unknown> | undefined;
  const roundUsage = stats.roundUsage as Record<string, unknown> | undefined;
  const roundUsageSnake = stats.round_usage as Record<string, unknown> | undefined;
  const contextUsage = stats.context_usage as Record<string, unknown> | undefined;
  const usageTotal = normalizePositiveTokenCount(usage?.total ?? usage?.total_tokens ?? usage?.totalTokens);
  if (usageTotal !== null) {
    return usageTotal;
  }
  const usageInput = normalizePositiveTokenCount(usage?.input ?? usage?.input_tokens ?? usage?.inputTokens);
  if (usageInput !== null) {
    return usageInput;
  }
  const roundUsageTotal = normalizePositiveTokenCount(
    roundUsage?.total ??
      roundUsage?.total_tokens ??
      roundUsage?.totalTokens ??
      roundUsageSnake?.total ??
      roundUsageSnake?.total_tokens ??
      roundUsageSnake?.totalTokens
  );
  if (roundUsageTotal !== null) {
    return roundUsageTotal;
  }
  const roundUsageInput = normalizePositiveTokenCount(
    roundUsage?.input ??
      roundUsage?.input_tokens ??
      roundUsage?.inputTokens ??
      roundUsageSnake?.input ??
      roundUsageSnake?.input_tokens ??
      roundUsageSnake?.inputTokens
  );
  if (roundUsageInput !== null) {
    return roundUsageInput;
  }
  const explicitContext = normalizePositiveTokenCount(
    stats.contextTokens ??
      stats.contextOccupancyTokens ??
      stats.context_occupancy_tokens ??
      stats.context_tokens ??
      stats.context_tokens_total ??
      contextUsage?.context_tokens ??
      contextUsage?.contextTokens
  );
  if (explicitContext !== null) {
    return explicitContext;
  }
  return normalizePositiveTokenCount(
    contextUsage?.context_tokens ??
      contextUsage?.contextTokens ??
      usage?.total ??
      usage?.total_tokens ??
      usage?.totalTokens
  );
};

const resolveAssistantLiveContextTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  const contextUsage = stats.context_usage as Record<string, unknown> | undefined;
  const explicitContext = normalizePositiveTokenCount(
    stats.contextTokens ??
      stats.contextOccupancyTokens ??
      stats.context_occupancy_tokens ??
      stats.context_tokens ??
      stats.context_tokens_total ??
      contextUsage?.context_tokens ??
      contextUsage?.contextTokens
  );
  return explicitContext ?? resolveAssistantContextTokens(stats);
};

export const resolveAssistantContextTotalTokens = (stats: ComposerContextStatsSource): number | null => {
  if (!stats) {
    return null;
  }
  const contextUsage = stats.context_usage as Record<string, unknown> | undefined;
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
  const contextUsage = session.context_usage as Record<string, unknown> | undefined;
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
  const contextUsage = session.context_usage as Record<string, unknown> | undefined;
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
