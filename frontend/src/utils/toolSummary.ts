// Normalize tool and skill summaries from different backend payload formats.

type UnknownRecord = Record<string, unknown>;

type AbilityItem = {
  name: string;
  description: string;
  kind?: AbilityKind;
  group?: AbilityGroupKey;
};

type AbilityKind = 'tool' | 'skill';

export type AbilityGroupKey =
  | 'skills'
  | 'mcp'
  | 'knowledge'
  | 'a2a'
  | 'user'
  | 'shared'
  | 'builtin';

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

const hasOwn = (record: UnknownRecord, key: string) =>
  Object.prototype.hasOwnProperty.call(record, key);

const pickName = (item: unknown): string => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  const obj = asRecord(item);
  return String(obj.runtime_name || obj.runtimeName || obj.name || obj.tool_name || obj.toolName || obj.id || '');
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

const normalizeAbilityKind = (value: unknown): AbilityKind =>
  String(value || '').trim().toLowerCase() === 'skill' ? 'skill' : 'tool';

const normalizeAbilityGroup = (value: unknown): AbilityGroupKey | undefined => {
  const normalized = String(value || '').trim().toLowerCase();
  if (
    normalized === 'skills' ||
    normalized === 'mcp' ||
    normalized === 'knowledge' ||
    normalized === 'a2a' ||
    normalized === 'user' ||
    normalized === 'shared' ||
    normalized === 'builtin'
  ) {
    return normalized;
  }
  return undefined;
};

const normalizeCatalogItems = (list: unknown): AbilityItem[] => {
  if (!Array.isArray(list)) {
    return [];
  }
  const items: AbilityItem[] = [];
  const indexMap = new Map<string, number>();
  list.forEach((item) => {
    if (!item || typeof item !== 'object') return;
    const obj = asRecord(item);
    const name = pickName(obj).trim();
    if (!name) return;
    const description = String(obj.description || obj.desc || obj.summary || '').trim();
    const kind = normalizeAbilityKind(obj.kind);
    const group = normalizeAbilityGroup(obj.group);
    const key = `${kind}:${name}`;
    if (indexMap.has(key)) {
      const existingIndex = indexMap.get(key);
      if (existingIndex !== undefined && !items[existingIndex].description && description) {
        items[existingIndex].description = description;
      }
      return;
    }
    items.push({ name, description, kind, group });
    indexMap.set(key, items.length - 1);
  });
  return items;
};

const readUnifiedAbilityItems = (payload: UnknownRecord): AbilityItem[] => {
  if (!hasOwn(payload, 'items') && !hasOwn(payload, 'itemList')) {
    return [];
  }
  return normalizeCatalogItems(payload.items || payload.itemList || []);
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
  const unifiedItems = readUnifiedAbilityItems(payload);
  if (unifiedItems.length || hasOwn(payload, 'items') || hasOwn(payload, 'itemList')) {
    return {
      tools: uniqNames(
        unifiedItems.filter((item) => item.kind !== 'skill').map((item) => item.name)
      ),
      skills: uniqNames(
        unifiedItems.filter((item) => item.kind === 'skill').map((item) => item.name)
      )
    };
  }
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
  const unifiedItems = readUnifiedAbilityItems(payload);
  if (unifiedItems.length || hasOwn(payload, 'items') || hasOwn(payload, 'itemList')) {
    return {
      tools: unifiedItems.filter((item) => item.kind !== 'skill'),
      skills: unifiedItems.filter((item) => item.kind === 'skill')
    };
  }
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

export const collectAbilityGroupDetails = (payload: UnknownRecord = {}) => {
  const grouped = {
    skills: [] as AbilityItem[],
    mcp: [] as AbilityItem[],
    knowledge: [] as AbilityItem[],
    a2a: [] as AbilityItem[],
    user: [] as AbilityItem[],
    shared: [] as AbilityItem[],
    builtin: [] as AbilityItem[]
  };
  const unifiedItems = readUnifiedAbilityItems(payload);
  if (unifiedItems.length || hasOwn(payload, 'items') || hasOwn(payload, 'itemList')) {
    unifiedItems.forEach((item) => {
      if (!item.group) return;
      grouped[item.group].push(item);
    });
    return grouped;
  }
  return {
    skills: normalizeAbilityItems(payload.skills || payload.skill_list || payload.skillList || []),
    mcp: normalizeAbilityItems(payload.mcp_tools || payload.mcpTools || []),
    knowledge: normalizeAbilityItems(payload.knowledge_tools || payload.knowledgeTools || []),
    a2a: normalizeAbilityItems(payload.a2a_tools || payload.a2aTools || []),
    user: normalizeAbilityItems(payload.user_tools || payload.userTools || []),
    shared: normalizeAbilityItems(payload.shared_tools || payload.sharedTools || []),
    builtin: normalizeAbilityItems(payload.builtin_tools || payload.builtinTools || [])
  };
};
