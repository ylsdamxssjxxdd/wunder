export const tokenLabel = (value) => Number(value) >= 1048576 ? `${Number(value) / 1048576}m` : `${Number(value) / 1024}k`;
export const validResult = (item) => item.status === "finished" && item.metrics?.target_reached === true;

export function comparisonSeries(items, selected, metric, axis) {
  const groups = new Map();
  for (const item of items) {
    const value = item.metrics?.[metric];
    if (!selected.has(item.id) || value == null || !Number.isFinite(Number(value))) continue;
    // Invalid measurements remain inspectable without joining a successful curve.
    const group = `${item.config.model_name}${item.simulated ? ` [sim] ${item.simulation_speed || "legacy"}` : ""} · c${item.config.concurrency || 1} · ${tokenLabel(item.config.output_tokens)}${validResult(item) ? "" : " ⚠"}`;
    if (!groups.has(group)) groups.set(group, []);
    groups.get(group).push({ value: [axis === "time" ? Date.parse(item.started_at) : item.config.input_tokens, Number(value)], run: item });
  }
  return [...groups].map(([name, data]) => ({
    name, type: "line", symbol: "circle", symbolSize: 9, connectNulls: false,
    data: data.sort((a, b) => a.value[0] - b.value[0] || a.run.started_at.localeCompare(b.run.started_at)),
    lineStyle: { width: 2, type: name.endsWith("⚠") ? "dashed" : "solid" },
  }));
}
