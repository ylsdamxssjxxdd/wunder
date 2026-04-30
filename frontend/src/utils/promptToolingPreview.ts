export type PromptToolingPreviewItem = {
  key: string;
  name: string;
  description: string;
  protocolName: string;
  kind: 'tool' | 'skill';
  group: string;
  source: string;
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
  runtimeName?: string;
  displayOnly?: boolean;
};

type PromptToolingAbilityMeta = Pick<PromptToolingPreviewItem, 'kind' | 'group' | 'source'>;

const SKILL_KEYWORDS = ['skill', 'skills', 'workflow', 'template', 'preset', 'agent preset', '技能'];
const MCP_KEYWORDS = ['mcp', 'connector', 'integration', 'endpoint'];
const KNOWLEDGE_KEYWORDS = [
  'knowledge',
  'knowledgebase',
  'knowledge base',
  'knowledge_base',
  'rag',
  'vector',
  'embedding',
  'document',
  'kb',
  '知识'
];

const matchesKeyword = (text: string, keywords: string[]): boolean => {
  const lowerText = text.toLowerCase();
  const normalizedText = normalizeKey(text);
  return keywords.some((keyword) => {
    const lowerKeyword = keyword.toLowerCase();
    if (lowerText.includes(lowerKeyword)) {
      return true;
    }
    const normalizedKeyword = normalizeKey(lowerKeyword);
    return Boolean(normalizedKeyword && normalizedText.includes(normalizedKeyword));
  });
};

export const inferPromptToolingAbilityMeta = (value: {
  name?: unknown;
  description?: unknown;
  protocolName?: unknown;
  runtimeName?: unknown;
}): PromptToolingAbilityMeta => {
  const name = cleanText(value.name);
  const description = cleanText(value.description);
  const protocolName = cleanText(value.protocolName);
  const runtimeName = cleanText(value.runtimeName);
  const text = [name, description, protocolName, runtimeName].filter(Boolean).join(' ');

  if (protocolName.includes('@') || runtimeName.includes('@') || matchesKeyword(text, MCP_KEYWORDS)) {
    return {
      kind: 'tool',
      group: 'mcp',
      source: 'mcp'
    };
  }

  if (matchesKeyword(text, KNOWLEDGE_KEYWORDS)) {
    return {
      kind: 'tool',
      group: 'knowledge',
      source: 'knowledge'
    };
  }

  if (matchesKeyword(text, SKILL_KEYWORDS)) {
    return {
      kind: 'skill',
      group: 'skills',
      source: 'skills'
    };
  }

  return {
    kind: 'tool',
    group: '',
    source: protocolName || name
  };
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

const resolveSelectedToolMeta = (
  selectedName: string,
  displayName: string,
  byName: Map<string, ToolMeta>,
  byProtocol: Map<string, ToolMeta>
): ToolMeta | null => {
  const normalizedSelected = normalizeKey(selectedName);
  const normalizedDisplay = normalizeKey(displayName);
  const candidates = [
    normalizedSelected ? byProtocol.get(normalizedSelected) : null,
    normalizedDisplay ? byProtocol.get(normalizedDisplay) : null,
    normalizedDisplay ? byName.get(normalizedDisplay) : null,
    normalizedSelected ? byName.get(normalizedSelected) : null
  ].filter((item): item is ToolMeta => Boolean(item));
  return candidates.find((item) => !item.displayOnly) || candidates[0] || null;
};

const resolveMappedProtocolNames = (
  selectedName: string,
  displayName: string,
  llmToolNameMap: Record<string, unknown>
): string[] => {
  const normalizedSelected = normalizeKey(selectedName);
  const normalizedDisplay = normalizeKey(displayName);
  const output: string[] = [];
  const seen = new Set<string>();
  Object.entries(llmToolNameMap).forEach(([protocolName, mappedDisplayName]) => {
    const normalizedProtocol = normalizeKey(protocolName);
    if (!normalizedProtocol || normalizedProtocol === normalizedSelected) {
      return;
    }
    const normalizedMapped = normalizeKey(mappedDisplayName);
    if (
      normalizedMapped &&
      (normalizedMapped === normalizedDisplay || normalizedMapped === normalizedSelected) &&
      !seen.has(protocolName)
    ) {
      seen.add(protocolName);
      output.push(protocolName);
    }
  });
  return output;
};

const buildPromptToolingDebugText = (tooling: Record<string, unknown>): string => {
  const modelRequest = asRecord(tooling.model_request);
  if (Object.keys(modelRequest).length > 0) {
    return safeStringify(modelRequest);
  }
  return safeStringify(tooling);
};

const buildPromptToolingItems = (tooling: Record<string, unknown>): PromptToolingPreviewItem[] => {
  const llmToolNameMap = asRecord(tooling.llm_tool_name_map);
  const selectedToolDisplayMap = asRecord(tooling.selected_tool_display_map);
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
  Object.entries(llmToolNameMap).forEach(([protocolName, displayName]) => {
    const normalizedProtocol = normalizeKey(protocolName);
    if (!normalizedProtocol || byProtocol.has(normalizedProtocol)) {
      return;
    }
    const fallbackName = cleanText(displayName) || protocolName;
    byProtocol.set(normalizedProtocol, {
      name: fallbackName,
      description: '',
      protocolName,
      displayOnly: true
    });
  });

  const used = new Set<string>();
  const items: PromptToolingPreviewItem[] = [];
  const appendItem = (displayName: string, tool?: ToolMeta | null, runtimeName?: string) => {
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
    const meta = inferPromptToolingAbilityMeta({
      name,
      description: tool?.description,
      protocolName,
      runtimeName: runtimeName || tool?.runtimeName
    });
    items.push({
      key: `${uniqueKey || 'tool'}-${items.length}`,
      name,
      description: cleanText(tool?.description),
      protocolName,
      kind: meta.kind,
      group: meta.group,
      source: meta.source
    });
  };

  for (const selectedName of selectedToolNames) {
    const displayName = cleanText(selectedToolDisplayMap[selectedName]) || selectedName;
    const tool = resolveSelectedToolMeta(selectedName, displayName, byName, byProtocol);
    const mappedProtocolNames = resolveMappedProtocolNames(selectedName, displayName, llmToolNameMap);
    appendItem(
      displayName,
      tool ? { ...tool, runtimeName: selectedName } : tool,
      [selectedName, ...mappedProtocolNames].filter(Boolean).join(' ')
    );
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
    text: buildPromptToolingDebugText(tooling),
    items: buildPromptToolingItems(tooling)
  };
};
