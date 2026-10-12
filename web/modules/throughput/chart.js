// AI生成
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

// Expand a single run into one point per concurrency level, each carrying that
// level's own metrics. Older single-concurrency records without samples fall
// back to the run-level summary.
export const samplePoints = (item) => {
  const samples = Array.isArray(item?.samples) ? item.samples : [];
  if (samples.length) {
    return samples.map((sample) => ({
      concurrency: sample.concurrency,
      status: sample.status || item.status,
      elapsed_s: sample.elapsed_s,
      metrics: sample.metrics || {},
    }));
  }
  const concurrency = (item?.config?.concurrency_list || [item?.config?.concurrency || 1])[0];
  return [{ concurrency, status: item?.status, elapsed_s: item?.elapsed_s, metrics: item?.metrics || {} }];
};

const METRICS = [
  ["prefill_tps", "prefill", "#2563eb"],
  ["avg_prefill_tps", "avgPrefill", "#8b5cf6"],
  ["decode_tps", "decode", "#16a34a"],
  ["avg_decode_tps", "avgDecode", "#d97706"],
];

// Build one series per metric from a single run's per-concurrency points. The
// y-axis shows the measured speed (tok/s), so each concurrency level reports
// its own result instead of a value relative to concurrency 1.
export function concurrencySeries(item, label) {
  if (!item) return [];
  const points = samplePoints(item);
  return METRICS.map(([key, labelKey, color]) => {
    const byConcurrency = new Map();
    for (const point of points) {
      const concurrency = Number(point.concurrency || 1);
      const value = Number(point.metrics?.[key]);
      if (Number.isFinite(concurrency) && Number.isFinite(value) && value >= 0) {
        byConcurrency.set(concurrency, value);
      }
    }
    const data = [...byConcurrency]
      .map(([concurrency, value]) => ({ concurrency, value }))
      .sort((left, right) => left.concurrency - right.concurrency);
    if (!data.length) return null;
    return {
      name: label(labelKey), type: "line", smooth: false, symbol: "circle", symbolSize: 8,
      lineStyle: { color, width: 2 }, itemStyle: { color },
      data: data.map((point) => ({ value: [point.concurrency, Number(point.value.toFixed(2))] })),
    };
  }).filter(Boolean);
}
