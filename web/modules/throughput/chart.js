export const tokenLabel = (value) => {
  const tokens = Number(value);
  if (!Number.isFinite(tokens)) return "—";
  const compact = (amount, suffix) => `${Number(amount.toFixed(1))}${suffix}`;
  if (tokens >= 100000) return compact(tokens / 100000, "m");
  if (tokens >= 1000) return compact(tokens / 1000, "k");
  return `${Math.round(tokens)}`;
};
export const speedLabel = (value) => {
  const speed = Number(value);
  if (!Number.isFinite(speed)) return "—";
  return `${tokenLabel(speed)} tok/s`;
};
export const selectableResult = (item) => ["finished", "incomplete"].includes(item.status);

const METRICS = [
  ["prefill_tps", "prefill", "#2563eb"],
  ["avg_prefill_tps", "avgPrefill", "#8b5cf6"],
  ["decode_tps", "decode", "#16a34a"],
  ["avg_decode_tps", "avgDecode", "#d97706"],
];

const sameScenario = (left, right) => left.config.model_name === right.config.model_name
  && Number(left.config.input_tokens) === Number(right.config.input_tokens)
  && Number(left.config.output_tokens) === Number(right.config.output_tokens)
  && Boolean(left.simulated) === Boolean(right.simulated)
  && (left.simulation_speed || "") === (right.simulation_speed || "");

export function latestScenario(items, selected) {
  const candidates = items.filter((item) => selected.has(item.id) && selectableResult(item));
  return candidates.at(-1) || null;
}

export function concurrencySeries(items, selected, label) {
  const scenario = latestScenario(items, selected);
  if (!scenario) return [];
  const runs = items
    .filter((item) => selected.has(item.id) && selectableResult(item) && sameScenario(item, scenario))
    .flatMap((item) => item.samples?.length
      ? item.samples
        .filter((sample) => !sample.status || selectableResult(sample))
        .map((sample) => ({ ...item, config: { ...item.config, concurrency: sample.concurrency }, metrics: sample.metrics, status: sample.status }))
      : [item]);
  return METRICS.map(([key, labelKey, color]) => {
    const pointsByConcurrency = new Map();
    for (const item of runs) {
      const concurrency = Number(item.config.concurrency || 1);
      const value = Number(item.metrics?.[key]);
      if (Number.isFinite(concurrency) && Number.isFinite(value) && value >= 0) {
        // Repeated selected runs use the latest snapshot, so each concurrency has one point.
        pointsByConcurrency.set(concurrency, value);
      }
    }
    const points = [...pointsByConcurrency].map(([concurrency, value]) => ({ concurrency, value }))
      .sort((left, right) => left.concurrency - right.concurrency);
    const baseline = points.find((point) => point.concurrency === 1)?.value;
    if (!Number.isFinite(baseline) || baseline <= 0) return null;
    return {
      name: label(labelKey), type: "line", smooth: false, symbol: "circle", symbolSize: 8,
      lineStyle: { color, width: 2 }, itemStyle: { color },
      data: points.map((point) => ({
        value: [point.concurrency, Number((((point.value - baseline) / baseline) * 100).toFixed(2))],
        raw: point.value,
      })),
    };
  }).filter(Boolean);
}
