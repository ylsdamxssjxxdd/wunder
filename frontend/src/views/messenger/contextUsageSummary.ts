import {
  formatContextTokenCount,
  resolveAssistantContextTokens,
  resolveAssistantContextTotalTokens,
  resolveSessionContextTokens,
  resolveSessionContextTotalTokens
} from '@/components/chat/composerContextUsage';

export type ContextUsageSummary = {
  usedTokens: number | null;
  totalTokens: number | null;
  usedLabel: string;
  totalLabel: string;
  ratio: number | null;
  hasUsage: boolean;
};

const EMPTY_SOURCE = { usedTokens: null, totalTokens: null } as const;

const build = (used: number | null, total: number | null): ContextUsageSummary => {
  const hasUsage = used !== null && total !== null && total > 0;
  return {
    usedTokens: used,
    totalTokens: total,
    usedLabel: formatContextTokenCount(used),
    totalLabel: formatContextTokenCount(total),
    ratio: hasUsage ? Math.min(1, (used as number) / (total as number)) : null,
    hasUsage
  };
};

/**
 * Context usage for the shell status surfaces. The durable session record is the
 * primary source; the running assistant stats cover the window before the record
 * is refreshed. This is a read-only projection: it never mutates chat state.
 */
export const resolveContextUsageSummary = (
  session: Record<string, unknown> | null | undefined,
  runningAssistantStats: Record<string, unknown> | null | undefined
): ContextUsageSummary => {
  const sessionUsed = resolveSessionContextTokens(session);
  const sessionTotal = resolveSessionContextTotalTokens(session);
  if (sessionUsed !== null && sessionTotal !== null) {
    return build(sessionUsed, sessionTotal);
  }
  const assistantUsed = resolveAssistantContextTokens(runningAssistantStats);
  const assistantTotal = resolveAssistantContextTotalTokens(runningAssistantStats);
  if (assistantUsed !== null && assistantTotal !== null) {
    return build(assistantUsed, assistantTotal);
  }
  return { ...EMPTY_SOURCE, usedLabel: '--', totalLabel: '--', ratio: null, hasUsage: false };
};

export const formatContextUsageText = (summary: ContextUsageSummary): string =>
  summary.hasUsage ? `${summary.usedLabel} / ${summary.totalLabel}` : '--';
