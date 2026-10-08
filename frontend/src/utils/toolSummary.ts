// Normalize tool and skill summaries from different backend payload formats.

type UnknownRecord = Record<string, unknown>;

type AbilityItem = {
  name: string;
  runtimeName: string;
  displayName: string;
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

const hasTypedUserGroups = (payload: UnknownRecord): boolean =>
  hasOwn(payload, 'user_mcp_tools') ||
  hasOwn(payload, 'userMcpTools') ||
  hasOwn(payload, 'user_skills') ||
  hasOwn(payload, 'userSkills') ||
  hasOwn(payload, 'user_knowledge_tools') ||
  hasOwn(payload, 'userKnowledgeTools');

const pickName = (item: unknown): string => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  const obj = asRecord(item);
  return String(obj.runtime_name || obj.runtimeName || obj.name || obj.tool_name || obj.toolName || obj.id || '');
};

const pickDisplayName = (item: unknown): string => {
  if (!item) return '';
  if (typeof item === 'string') return item;
  const obj = asRecord(item);
  return String(
    obj.display_name ||
      obj.displayName ||
      obj.title ||
      obj.label ||
      obj.runtime_name ||
      obj.runtimeName ||
      obj.name ||
      obj.tool_name ||
      obj.toolName ||
      obj.id ||
      ''
  );
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
    const runtimeName = pickName(item).trim();
    if (!runtimeName) return;
    const displayName = pickDisplayName(item).trim() || runtimeName;
    const description =
      typeof item === 'string' ? '' : String(asRecord(item).description || asRecord(item).desc || asRecord(item).summary || '').trim();
    if (indexMap.has(runtimeName)) {
      const existingIndex = indexMap.get(runtimeName);
      if (existingIndex !== undefined) {
        if (!items[existingIndex].description && description) {
          items[existingIndex].description = description;
        }
        if (!items[existingIndex].displayName && displayName) {
          items[existingIndex].displayName = displayName;
        }
      }
      return;
    }
    items.push({
      name: runtimeName,
      runtimeName,
      displayName,
      description
    });
    indexMap.set(runtimeName, items.length - 1);
  });
  return items;
};

const normalizeAbilityKind = (value: unknown): AbilityKind =>
  String(value || '').trim().toLowerCase() === 'skill' ? 'skill' : 'tool';

const normalizeAbilitySource = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const normalizeAbilityGroup = (
  value: unknown,
  source: unknown = '',
  kind: AbilityKind = 'tool'
): AbilityGroupKey | undefined => {
  const normalized = String(value || '').trim().toLowerCase();
  const normalizedSource = normalizeAbilitySource(source);
  if (normalizedSource === 'user_mcp') {
    return 'mcp';
  }
  if (normalizedSource === 'user_skill') {
    return 'skills';
  }
  if (normalizedSource === 'user_knowledge') {
    return 'knowledge';
  }
  if (normalized === 'user' && kind === 'skill') {
    return 'skills';
  }
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

const mergeAbilityItems = (...lists: Array<AbilityItem[] | undefined>): AbilityItem[] => {
  const items: AbilityItem[] = [];
  const indexMap = new Map<string, number>();
  for (const list of lists) {
    if (!Array.isArray(list)) {
      continue;
    }
    for (const item of list) {
      const name = String(item?.name || '').trim();
      if (!name) {
        continue;
      }
      const description = String(item?.description || '').trim();
      const existingIndex = indexMap.get(name);
      if (existingIndex !== undefined) {
        if (!items[existingIndex].description && description) {
          items[existingIndex].description = description;
        }
        if (!items[existingIndex].displayName && item?.displayName) {
          items[existingIndex].displayName = item.displayName;
        }
        continue;
      }
      const runtimeName = String(item?.runtimeName || item?.name || '').trim() || name;
      const displayName = String(item?.displayName || runtimeName).trim() || runtimeName;
      items.push({
        name,
        runtimeName,
        displayName,
        description,
        kind: item?.kind,
        group: item?.group
      });
      indexMap.set(name, items.length - 1);
    }
  }
  return items;
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
    const runtimeName = pickName(obj).trim();
    if (!runtimeName) return;
    const displayName = pickDisplayName(obj).trim() || runtimeName;
    const description = String(obj.description || obj.desc || obj.summary || '').trim();
    const kind = normalizeAbilityKind(obj.kind);
    const group = normalizeAbilityGroup(obj.group, obj.source, kind);
    const key = `${kind}:${runtimeName}`;
    if (indexMap.has(key)) {
      const existingIndex = indexMap.get(key);
      if (existingIndex !== undefined) {
        if (!items[existingIndex].description && description) {
          items[existingIndex].description = description;
        }
        if (!items[existingIndex].displayName && displayName) {
          items[existingIndex].displayName = displayName;
        }
      }
      return;
    }
    items.push({
      name: runtimeName,
      runtimeName,
      displayName,
      description,
      kind,
      group
    });
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
  const typedUserGroups = hasTypedUserGroups(payload);
  const legacyUserTools = typedUserGroups
    ? []
    : normalizeToolNames(payload.user_tools || payload.userTools);
  const toolGroups = [
    payload.builtin_tools || payload.builtinTools,
    payload.mcp_tools || payload.mcpTools,
    payload.a2a_tools || payload.a2aTools,
    payload.knowledge_tools || payload.knowledgeTools,
    payload.user_mcp_tools || payload.userMcpTools,
    payload.user_knowledge_tools || payload.userKnowledgeTools,
    legacyUserTools,
    payload.shared_tools || payload.sharedTools
  ];
  const tools = uniqNames(toolGroups.flatMap((list) => normalizeToolNames(list)));
  const skills = uniqNames(
    [
      ...normalizeToolNames(payload.skills || payload.skill_list || payload.skillList || []),
      ...normalizeToolNames(payload.user_skills || payload.userSkills || [])
    ]
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
  const typedUserGroups = hasTypedUserGroups(payload);
  const legacyUserTools = typedUserGroups
    ? []
    : normalizeAbilityItems(payload.user_tools || payload.userTools || []);
  const tools = mergeAbilityItems(
    normalizeAbilityItems(payload.builtin_tools || payload.builtinTools || []),
    normalizeAbilityItems(payload.mcp_tools || payload.mcpTools || []),
    normalizeAbilityItems(payload.a2a_tools || payload.a2aTools || []),
    normalizeAbilityItems(payload.knowledge_tools || payload.knowledgeTools || []),
    normalizeAbilityItems(payload.user_mcp_tools || payload.userMcpTools || []),
    normalizeAbilityItems(payload.user_knowledge_tools || payload.userKnowledgeTools || []),
    legacyUserTools,
    normalizeAbilityItems(payload.shared_tools || payload.sharedTools || [])
  );
  const skills = mergeAbilityItems(
    normalizeAbilityItems(payload.skills || payload.skill_list || payload.skillList || []),
    normalizeAbilityItems(payload.user_skills || payload.userSkills || [])
  );
  return { tools, skills };
};

export const collectAbilityGroupDetails = (payload: UnknownRecord = {}) => {
  const typedUserGroups = hasTypedUserGroups(payload);
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
    skills: mergeAbilityItems(
      normalizeAbilityItems(payload.skills || payload.skill_list || payload.skillList || []),
      normalizeAbilityItems(payload.user_skills || payload.userSkills || [])
    ),
    mcp: mergeAbilityItems(
      normalizeAbilityItems(payload.mcp_tools || payload.mcpTools || []),
      normalizeAbilityItems(payload.user_mcp_tools || payload.userMcpTools || [])
    ),
    knowledge: mergeAbilityItems(
      normalizeAbilityItems(payload.knowledge_tools || payload.knowledgeTools || []),
      normalizeAbilityItems(payload.user_knowledge_tools || payload.userKnowledgeTools || [])
    ),
    a2a: normalizeAbilityItems(payload.a2a_tools || payload.a2aTools || []),
    user: typedUserGroups ? [] : normalizeAbilityItems(payload.user_tools || payload.userTools || []),
    shared: normalizeAbilityItems(payload.shared_tools || payload.sharedTools || []),
    builtin: normalizeAbilityItems(payload.builtin_tools || payload.builtinTools || [])
  };
};
