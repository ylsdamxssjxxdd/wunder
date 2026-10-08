export type RuntimeHeatmapSourceItem = {
  tool?: unknown;
  name?: unknown;
  display_name?: unknown;
  displayName?: unknown;
  tool_name?: unknown;
  toolName?: unknown;
  runtime_name?: unknown;
  runtimeName?: unknown;
  category?: unknown;
  group?: unknown;
  source?: unknown;
  total_calls?: unknown;
};

export type RuntimeHeatmapItem = {
  tool: string;
  runtimeName: string;
  category: string;
  group: string;
  source: string;
  total_calls: number;
};

const cleanText = (value: unknown): string => String(value || '').trim();

const toSafeNumber = (value: unknown): number => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
};

export const resolveRuntimeHeatmapCategory = (
  item: RuntimeHeatmapSourceItem,
  runtimeName: string
): string => {
  const category = cleanText(item?.category || item?.group || item?.source).toLowerCase();
  if (category.includes('mcp')) {
    return 'mcp';
  }
  if (category.includes('knowledge')) {
    return 'knowledge';
  }
  if (category.includes('skill')) {
    return 'skill';
  }
  const runtime = cleanText(runtimeName).toLowerCase();
  if (runtime.startsWith('a2a@')) {
    return 'a2a';
  }
  if (runtime.includes('@')) {
    return 'mcp';
  }
  if (runtime.startsWith('kb_') || runtime.includes('knowledge') || runtime.includes('rag')) {
    return 'knowledge';
  }
  return category || 'other';
};

export const normalizeRuntimeHeatmapItems = (source: unknown): RuntimeHeatmapItem[] => {
  const items = Array.isArray(source) ? source : [];
  const merged = new Map<string, RuntimeHeatmapItem>();
  for (const raw of items) {
    const item = (raw || {}) as RuntimeHeatmapSourceItem;
    const displayName = cleanText(
      item.display_name || item.displayName || item.tool || item.name
    );
    const runtimeName = cleanText(
      item.runtime_name || item.runtimeName || item.tool_name || item.toolName
    );
    const key = runtimeName || displayName || 'unknown';
    const totalCalls = Math.max(0, toSafeNumber(item.total_calls));
    const category = resolveRuntimeHeatmapCategory(item, runtimeName || displayName);
    const existing = merged.get(key);
    if (existing) {
      existing.total_calls += totalCalls;
      if ((existing.tool === key || existing.tool === 'unknown') && displayName) {
        existing.tool = displayName;
      }
      if (existing.category === 'other' && category !== 'other') {
        existing.category = category;
        existing.group = existing.group || category;
        existing.source = existing.source || category;
      }
      continue;
    }
    merged.set(key, {
      tool: displayName || runtimeName || 'unknown',
      runtimeName: runtimeName || displayName || 'unknown',
      category,
      group: cleanText(item.group || category),
      source: cleanText(item.source || category),
      total_calls: totalCalls
    });
  }
  return Array.from(merged.values()).sort((left, right) => {
    const byCalls = right.total_calls - left.total_calls;
    return byCalls || left.tool.localeCompare(right.tool);
  });
};
