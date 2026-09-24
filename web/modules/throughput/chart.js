export const tokenLabel = (value) => {
  const tokens = Number(value);
  if (!Number.isFinite(tokens)) return "—";
  if (tokens >= 1048576) return `${(tokens / 1048576).toFixed(tokens % 1048576 ? 1 : 0)}m`;
  if (tokens >= 1024) return `${(tokens / 1024).toFixed(tokens % 1024 ? 1 : 0)}k`;
  return `${Math.round(tokens)}`;
};
export const selectableResult = (item) => ["finished", "incomplete"].includes(item.status);

export function comparisonSeries(items, selected, metric, axis) {
  const groups = new Map();
  for (const item of items) {
    const value = item.metrics?.[metric];
    if (!selected.has(item.id) || value == null || !Number.isFinite(Number(value))) continue;
    // Keep every selected measurement inspectable without exposing an outcome judgment.
    const group = `${item.config.model_name}${item.simulated ? ` [sim] ${item.simulation_speed || "legacy"}` : ""} · c${item.config.concurrency || 1} · ${tokenLabel(item.config.output_tokens)}`;
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group).push({ value: [axis === "time" ? Date.parse(item.started_at) : item.config.input_tokens, Number(value)], run: item });
  }
  return [...groups].map(([name, data]) => ({
    name, type: "line", symbol: "circle", symbolSize: 9, connectNulls: false,
    data: data.sort((a, b) => a.value[0] - b.value[0] || a.run.started_at.localeCompare(b.run.started_at)),
    lineStyle: { width: 2, type: "solid" },
  }));
}
