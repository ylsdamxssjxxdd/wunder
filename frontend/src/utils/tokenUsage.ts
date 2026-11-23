export type TokenUsageSnapshot = {
  input: number;
  output: number;
  total: number;
  reasoning?: number;
  estimated?: boolean;
};

const countTokens = (raw: unknown): number | null => {
  if (raw === null || raw === undefined || raw === '') return null;
  const parsed = Number(raw);
  return Number.isFinite(parsed) && parsed >= 0 ? Math.trunc(parsed) : null;
};

/**
 * Prompt size as the provider saw it: uncached input + cache reads + cache writes.
 *
 * `input_tokens` is only the **uncached** share once usage is split into three
 * disjoint buckets, so any number that means "how full is the context" must sum
 * the buckets back together. Provider-native inclusive `prompt_tokens` and
 * Responses `input_tokens` are used as-is.
 *
 * Prefer this over reading `usage.input_tokens` directly for context occupancy,
 * composer gauges and per-round "context tokens" labels.
 */
export const resolvePromptTokens = (value: unknown): number | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const source = value as Record<string, unknown>;
  const inputDetails = (source.input_tokens_details ?? source.inputTokensDetails) as
    | Record<string, unknown>
    | undefined;
  const inclusive = countTokens(
    source.prompt_tokens ?? source.promptTokens ?? source.prompt
  );
  const input = countTokens(source.input_tokens ?? source.inputTokens ?? source.input);
  const cacheRead = countTokens(
    source.cache_read_tokens ??
      source.cache_read_input_tokens ??
      source.cacheReadTokens ??
      source.cacheReadInputTokens ??
      source.cached_input_tokens ??
      source.cached_tokens ??
      inputDetails?.cached_tokens ??
      inputDetails?.cachedTokens ??
      source.prompt_cache_hit_tokens
  );
  const cacheWrite = countTokens(
    source.cache_write_tokens ??
      source.cache_creation_input_tokens ??
      source.cacheWriteTokens ??
      source.cacheCreationInputTokens ??
      inputDetails?.cache_write_tokens ??
      inputDetails?.cacheWriteTokens
  );
  if (inclusive !== null || (inputDetails && input !== null)) {
    // Responses input_tokens and Chat Completions prompt_tokens already include
    // the cache buckets; never add them twice.
    return Math.max(inclusive ?? input ?? 0, input ?? 0);
  }
  if (input === null && cacheRead === null && cacheWrite === null) return null;
  return (input ?? 0) + (cacheRead ?? 0) + (cacheWrite ?? 0);
};

export const normalizeTokenUsage = (value: unknown): TokenUsageSnapshot | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const source = value as Record<string, unknown>;
  const count = countTokens;
  const input = count(source.input_tokens ?? source.prompt_tokens ?? source.inputTokens ?? source.promptTokens ?? source.input ?? source.prompt);
  const output = count(source.output_tokens ?? source.completion_tokens ?? source.outputTokens ?? source.completionTokens ?? source.output ?? source.completion);
  const total = count(source.total_tokens ?? source.totalTokens ?? source.total);
  const reasoning = count(source.reasoning_tokens ?? source.reasoning);
  if (input === null && output === null && total === null) return null;
  // Explicit zero is authoritative, especially for reasoning-only responses.
  // Only legacy payloads lacking output entirely may derive an unclassified remainder.
  const normalizedOutput = output ?? Math.max(0, (total ?? 0) - (input ?? 0) - (reasoning ?? 0));
  return {
    input: input ?? 0,
    output: normalizedOutput,
    total: Math.max(total ?? 0, (input ?? 0) + normalizedOutput + (reasoning ?? 0)),
    ...(reasoning !== null ? { reasoning } : {}),
    ...(typeof source.estimated === 'boolean' ? { estimated: source.estimated } : {})
  };
};
