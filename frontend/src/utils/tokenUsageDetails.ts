import { normalizeTokenUsage } from './tokenUsage';

export const tokenUsageDetails = (
  stats: Record<string, unknown>,
  t: (key: string, params?: Record<string, unknown>) => string
): string => {
  const usage = normalizeTokenUsage(stats.roundUsage ?? stats.round_usage);
  if (!usage) return t('chat.stats.quotaHint');
  return t('chat.stats.usageBreakdown', {
    input: usage.input, output: usage.output,
    reasoning: usage.reasoning ?? t('chat.stats.unreported'), total: usage.total,
    source: t(usage.estimated ? 'chat.stats.estimated' : 'chat.stats.providerReported')
  });
};
