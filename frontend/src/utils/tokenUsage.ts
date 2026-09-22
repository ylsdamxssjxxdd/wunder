export type TokenUsageSnapshot = {
  input: number;
  output: number;
  total: number;
  reasoning?: number;
  estimated?: boolean;
};

export const normalizeTokenUsage = (value: unknown): TokenUsageSnapshot | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const source = value as Record<string, unknown>;
  const count = (raw: unknown): number | null => {
    if (raw === null || raw === undefined || raw === '') return null;
    const parsed = Number(raw);
    return Number.isFinite(parsed) && parsed >= 0 ? Math.trunc(parsed) : null;
  };
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
