export type PromptToolingPreviewItem = {
  key: string;
  name: string;
  description: string;
  protocolName: string;
};

export type PromptToolingPreview = {
  mode: string;
  text: string;
  items: PromptToolingPreviewItem[];
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

const cleanText = (value: unknown): string => String(value || '').trim();

const normalizeKey = (value: unknown): string =>
  cleanText(value)
    .toLowerCase()
    .replace(/[\s_.\-:/\\@]+/g, '');

const normalizeMode = (value: unknown): string => {
  const mode = cleanText(value).toLowerCase();
  if (mode === 'function_call' || mode === 'tool_call' || mode === 'freeform_call') {
    return mode;
  }
  return '';
};

const asStringArray = (value: unknown): string[] =>
  Array.isArray(value)
    ? value
        .map((item) => cleanText(item))
        .filter(Boolean)
    : [];

const safeStringify = (value: unknown): string => {
  const seen = new WeakSet<object>();
  try {
    const text = JSON.stringify(
      value,
      (_key, current: unknown) => {
        if (typeof current === 'bigint') {
          return current.toString();
        }
        if (current && typeof current === 'object') {
          const objectValue = current as object;
          if (seen.has(objectValue)) {
            return '[Circular]';
          }
          seen.add(objectValue);
          if (current instanceof Map) {
            return Object.fromEntries(current.entries());
          }
          if (current instanceof Set) {
            return Array.from(current.values());
          }
        }
        return current;
      },
      2
    );
    return typeof text === 'string' ? text : '';
  } catch {
    return '';
  }
};

type ToolMeta = {
  name: string;
  description: string;
  protocolName: string;
};

const extractLlmToolMeta = (value: unknown): ToolMeta | null => {
  const tool = asRecord(value);
  const type = cleanText(tool.type).toLowerCase();
  if (type === 'function') {
    const functionSpec = asRecord(tool.function);
    const protocolName = cleanText(functionSpec.name);
    if (!protocolName) {
      return null;
    }
    return {
      name: protocolName,
      description: cleanText(functionSpec.description),
      protocolName
    };
  }
  const protocolName = cleanText(tool.name);
  if (!protocolName) {
    return null;
  }
  return {
    name: protocolName,
    description: cleanText(tool.description),
    protocolName
  };
};

const buildPromptToolingItems = (tooling: Record<string, unknown>): PromptToolingPreviewItem[] => {
  const llmToolNameMap = asRecord(tooling.llm_tool_name_map);
  const llmTools = Array.isArray(tooling.llm_tools) ? tooling.llm_tools : [];
  const selectedToolNames = asStringArray(tooling.selected_tool_names);
  const resolvedTools = llmTools
    .map((tool) => extractLlmToolMeta(tool))
    .filter((tool): tool is ToolMeta => Boolean(tool))
    .map((tool) => ({
      ...tool,
      name: cleanText(llmToolNameMap[tool.protocolName]) || tool.name || tool.protocolName
    }));

  const byName = new Map<string, ToolMeta>();
  const byProtocol = new Map<string, ToolMeta>();
  for (const tool of resolvedTools) {
    const normalizedName = normalizeKey(tool.name);
    const normalizedProtocol = normalizeKey(tool.protocolName);
    if (normalizedName && !byName.has(normalizedName)) {
      byName.set(normalizedName, tool);
    }
    if (normalizedProtocol && !byProtocol.has(normalizedProtocol)) {
      byProtocol.set(normalizedProtocol, tool);
    }
  }

  const used = new Set<string>();
  const items: PromptToolingPreviewItem[] = [];
  const appendItem = (displayName: string, tool?: ToolMeta | null) => {
    const name = cleanText(displayName) || cleanText(tool?.name) || cleanText(tool?.protocolName);
    if (!name) {
      return;
    }
    const protocolName = cleanText(tool?.protocolName);
    const uniqueKey = normalizeKey(protocolName || name);
    if (uniqueKey && used.has(uniqueKey)) {
      return;
    }
    if (uniqueKey) {
      used.add(uniqueKey);
    }
    items.push({
      key: `${uniqueKey || 'tool'}-${items.length}`,
      name,
      description: cleanText(tool?.description),
      protocolName
    });
  };

  for (const selectedName of selectedToolNames) {
    const normalizedSelected = normalizeKey(selectedName);
    appendItem(selectedName, byName.get(normalizedSelected) || byProtocol.get(normalizedSelected));
  }

  for (const tool of resolvedTools) {
    appendItem(tool.name, tool);
  }

  return items;
};

export const extractPromptToolingPreview = (payload: unknown): PromptToolingPreview => {
  const source = asRecord(payload);
  const tooling = asRecord(source.tooling_preview);
  if (!Object.keys(tooling).length) {
    return { mode: '', text: '', items: [] };
  }
  return {
    mode: normalizeMode(tooling.tool_call_mode),
    text: safeStringify(tooling),
    items: buildPromptToolingItems(tooling)
  };
};
