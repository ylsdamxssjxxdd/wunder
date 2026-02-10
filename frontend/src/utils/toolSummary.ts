// 工具与技能名称汇总工具：统一解析接口返回的名称字段

type AnyRecord = Record<string, any>;

type AbilityItem = {
  name: string;
  description: string;
};

const normalizeToolNames = (list: unknown): string[] => {
  if (!Array.isArray(list)) {
    return [];
  }
  return list
    .map((item) => {
      if (!item) return '';
      if (typeof item === 'string') return item;
      const obj = item as AnyRecord;
      return obj.name || obj.tool_name || obj.toolName || obj.id || '';
    })
    .map((name) => String(name).trim())
    .filter(Boolean);
};

// 统一整理工具与技能的名称、描述，便于悬浮层展示详细信息
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
      const obj = item as AnyRecord;
      name = obj.name || obj.tool_name || obj.toolName || obj.id || '';
      description = obj.description || obj.desc || obj.summary || '';
    }
    const normalizedName = String(name).trim();
    if (!normalizedName) return;
    const normalizedDesc = String(description || '').trim();
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

// 收集工具与技能名称，输出去重后的列表
export const collectAbilityNames = (payload: AnyRecord = {}) => {
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

// 输出带描述的工具与技能列表，保持顺序并去重
export const collectAbilityDetails = (payload: AnyRecord = {}) => {
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
