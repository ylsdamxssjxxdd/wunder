import { normalizeChatDurationSeconds } from './chatTiming';

export type MessageStatsEntry = {
  label: string;
  value: string;
};

type TranslateFn = (key: string) => string;

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

export const buildAssistantMessageStatsEntries = (
  message: Record<string, any> | null | undefined,
  t: TranslateFn
): MessageStatsEntry[] => {
  if (!message || message.role !== 'assistant' || message.isGreeting || isAssistantStreaming(message)) {
    return [];
  }
  const stats = (message.stats || null) as Record<string, any> | null;
  if (!stats) return [];
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
    return [];
  }
  return [
    { label: t('chat.stats.duration'), value: formatDuration(durationSeconds) },
    { label: t('chat.stats.speed'), value: formatSpeed(speed) },
    { label: t('chat.stats.contextTokens'), value: formatCount(contextTokens) },
    { label: t('chat.stats.toolCalls'), value: formatCount(stats?.toolCalls) }
  ];
};
