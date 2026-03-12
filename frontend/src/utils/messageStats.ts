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

const MIN_SPEED_DURATION_S = 0.2;
const MAX_REASONABLE_SPEED = 10000;

const normalizeSpeed = (speed: number, durationSeconds: number | null): number | null => {
  if (!Number.isFinite(speed) || speed <= 0) return null;
  if (durationSeconds !== null && durationSeconds < MIN_SPEED_DURATION_S) return null;
  if (speed > MAX_REASONABLE_SPEED) return null;
  return speed;
};

const normalizeDurationSeconds = (value: unknown): number | null => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
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

const resolveTokenSpeed = (stats: Record<string, any>, durationSeconds: number | null): number | null => {
  const averageSpeed = Number(
    stats?.avg_model_round_speed_tps ??
      stats?.avgModelRoundSpeedTps ??
      stats?.average_speed_tps ??
      stats?.averageSpeedTps
  );
  const averageRounds = Number(
    stats?.avg_model_round_speed_rounds ??
      stats?.avgModelRoundSpeedRounds ??
      stats?.average_speed_rounds ??
      stats?.averageSpeedRounds
  );
  if (
    Number.isFinite(averageSpeed) &&
    averageSpeed > 0 &&
    (!Number.isFinite(averageRounds) || averageRounds > 0)
  ) {
    const speed = normalizeSpeed(averageSpeed, null);
    if (speed !== null) return speed;
  }
  const outputTokens = Number(stats?.usage?.output);
  const decode = normalizeDurationSeconds(
    stats?.decode_duration_total_s ??
      stats?.decodeDurationTotalS ??
      stats?.decode_duration_s
  );
  if (Number.isFinite(outputTokens) && outputTokens > 0 && decode !== null && decode > 0) {
    const speed = normalizeSpeed(outputTokens / decode, decode);
    if (speed !== null) return speed;
  }
  if (Number.isFinite(outputTokens) && outputTokens > 0 && durationSeconds && durationSeconds > 0) {
    const speed = normalizeSpeed(outputTokens / durationSeconds, durationSeconds);
    if (speed !== null) return speed;
  }
  const totalTokens = Number(stats?.usage?.total);
  if (Number.isFinite(totalTokens) && totalTokens > 0 && durationSeconds && durationSeconds > 0) {
    return normalizeSpeed(totalTokens / durationSeconds, durationSeconds);
  }
  return null;
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
  const speed = resolveTokenSpeed(stats, durationSeconds);
  const contextTokens =
    stats?.contextTokens ??
    stats?.context_tokens ??
    stats?.context_usage?.context_tokens ??
    stats?.context_usage?.contextTokens ??
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
