type RoundDecodeSpeedMetric = {
  prefill?: unknown;
  decode?: unknown;
  usage?: {
    output?: unknown;
  } | null;
} | null | undefined;

export type TurnDecodeSpeedSummary = {
  prefillDurationTotalS: number | null;
  decodeDurationTotalS: number | null;
  avgModelRoundSpeedTps: number | null;
  avgModelRoundSpeedRounds: number;
};

const normalizePositiveDuration = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const normalizePositiveTokens = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const summarizeTurnDecodeSpeed = (
  metrics: Iterable<RoundDecodeSpeedMetric>
): TurnDecodeSpeedSummary => {
  let prefillDurationTotalS = 0;
  let decodeDurationTotalS = 0;
  let decodeTokensTotal = 0;
  let avgModelRoundSpeedRounds = 0;
  let hasPrefillDuration = false;

  for (const metric of metrics) {
    const prefill = normalizePositiveDuration(metric?.prefill);
    if (prefill !== null) {
      prefillDurationTotalS += prefill;
      hasPrefillDuration = true;
    }

    const decode = normalizePositiveDuration(metric?.decode);
    const outputTokens = normalizePositiveTokens(metric?.usage?.output);
    if (decode === null || outputTokens === null) {
      continue;
    }
    decodeDurationTotalS += decode;
    decodeTokensTotal += outputTokens;
    avgModelRoundSpeedRounds += 1;
  }

  return {
    prefillDurationTotalS: hasPrefillDuration ? prefillDurationTotalS : null,
    decodeDurationTotalS: decodeDurationTotalS > 0 ? decodeDurationTotalS : null,
    avgModelRoundSpeedTps:
      decodeTokensTotal > 0 && decodeDurationTotalS > 0
        ? decodeTokensTotal / decodeDurationTotalS
        : null,
    avgModelRoundSpeedRounds
  };
};
