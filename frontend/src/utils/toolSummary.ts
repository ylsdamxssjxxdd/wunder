// Normalize tool and skill summaries from different backend payload formats.

type UnknownRecord = Record<string, unknown>;

type AbilityItem = {
  name: string;
  description: string;
};

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

const pickName = (item: unknown): string => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  const obj = asRecord(item);
  return String(obj.name || obj.tool_name || obj.toolName || obj.id || '');
};

const normalizeToolNames = (list: unknown): string[] => {
  if (!Array.isArray(list)) {
    return [];
  }
  return list.map((item) => pickName(item).trim()).filter(Boolean);
};

const normalizeAbilityItems = (list: unknown): AbilityItem[] => {
  if (!Array.isArray(list)) {
    return [];
  }
  const items: AbilityItem[] = [];
  const indexMap = new Map<string, number>();
  list.forEach((item) => {
    if (!item) return;
    let name = '';
    let description = '';
    if (typeof item === 'string') {
      name = item;
    } else {
      const obj = asRecord(item);
      name = String(obj.name || obj.tool_name || obj.toolName || obj.id || '');
      description = String(obj.description || obj.desc || obj.summary || '');
    }
    const normalizedName = name.trim();
    if (!normalizedName) return;
    const normalizedDesc = description.trim();
    if (indexMap.has(normalizedName)) {
      const existingIndex = indexMap.get(normalizedName);
      if (existingIndex !== undefined && !items[existingIndex].description && normalizedDesc) {
        items[existingIndex].description = normalizedDesc;
      }
      return;
    }
    items.push({ name: normalizedName, description: normalizedDesc });
    indexMap.set(normalizedName, items.length - 1);
  });
  return items;
};

const uniqNames = (names: string[]): string[] => {
  const seen = new Set<string>();
  return names.filter((name) => {
    if (seen.has(name)) return false;
    seen.add(name);
    return true;
  });
};

export const collectAbilityNames = (payload: UnknownRecord = {}) => {
  const toolGroups = [
    payload.builtin_tools || payload.builtinTools,
    payload.mcp_tools || payload.mcpTools,
    payload.a2a_tools || payload.a2aTools,
    payload.knowledge_tools || payload.knowledgeTools,
    payload.user_tools || payload.userTools,
    payload.shared_tools || payload.sharedTools
  ];
  const tools = uniqNames(toolGroups.flatMap((list) => normalizeToolNames(list)));
  const skills = uniqNames(
    normalizeToolNames(payload.skills || payload.skill_list || payload.skillList || [])
  );
  return { tools, skills };
};

export const collectAbilityDetails = (payload: UnknownRecord = {}) => {
  const toolGroups = [
    payload.builtin_tools || payload.builtinTools,
    payload.mcp_tools || payload.mcpTools,
    payload.a2a_tools || payload.a2aTools,
    payload.knowledge_tools || payload.knowledgeTools,
    payload.user_tools || payload.userTools,
    payload.shared_tools || payload.sharedTools
  ];
  const tools = normalizeAbilityItems(
    toolGroups.flatMap((list) => (Array.isArray(list) ? list : []))
  );
  const skills = normalizeAbilityItems(payload.skills || payload.skill_list || payload.skillList || []);
  return { tools, skills };
};
