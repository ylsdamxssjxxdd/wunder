/** 与折叠工具行同一形状的右侧指标：上下文占用 token 与条目自身耗时。 */
export type ModelRoundMetrics = { tokens: number | null; durationMs: number | null };

type UnknownRecord = Record<string, unknown>;

const isRecord = (value: unknown): value is UnknownRecord =>
  Boolean(value) && typeof value === 'object' && !Array.isArray(value);

const positive = (value: unknown): number | null => {
  const parsed = typeof value === 'number'
    ? value
    : typeof value === 'string' && value.trim() ? Number(value) : NaN;
  return Number.isFinite(parsed) && parsed > 0 ? Math.trunc(parsed) : null;
};

/**
 * 思考条目的右侧两列，与同轮工具条目同口径：
 * - token：该次模型请求的上下文占用（provider 报告的输入 token），
 *   依次取 `request_context_tokens` / `context_occupancy_tokens` / `usage.input_tokens`；
 * - 耗时：思考本身的跨度 `stream_timing.reasoning_ms`（首个到最后一个 reasoning 增量），
 *   不是整轮解码时间。
 *
 * 数据来自该轮 assistant 条目的载荷；线程视图把它包在 `payload` 里，
 * 实时投影直接放在消息原文上，两者都接受。旧记录没有 `reasoning_ms`
 * 时只显示 token，不拿整轮耗时冒充思考耗时。
 */
export const resolveModelRoundMetrics = (
  source: unknown
): ModelRoundMetrics | null => {
  if (!isRecord(source)) return null;
  const payload = isRecord(source.payload) ? { ...source, ...source.payload } : source;
  const usage = isRecord(payload.usage) ? payload.usage : null;
  const roundUsage = isRecord(payload.round_usage) ? payload.round_usage : null;
  const tokens =
    positive(payload.request_context_tokens) ??
    positive(payload.context_occupancy_tokens) ??
    positive(usage?.input_tokens) ??
    positive(usage?.inputTokens) ??
    positive(roundUsage?.input_tokens) ??
    null;
  const timing = isRecord(payload.stream_timing)
    ? payload.stream_timing
    : isRecord(payload.streamTiming) ? payload.streamTiming : null;
  const durationMs =
    positive(timing?.reasoning_ms) ?? positive(timing?.reasoningMs) ?? null;
  if (tokens === null && durationMs === null) return null;
  return { tokens, durationMs };
};
