export type ToolOptionLike = {
  label: string;
  value: string;
  description: string;
  hint: string;
};

export type AgentToolGroup<T extends ToolOptionLike = ToolOptionLike> = {
  key: string;
  label: string;
  options: T[];
};

export type AgentToolSection<T extends ToolOptionLike = ToolOptionLike> = {
  key: string;
  label: string;
  groups: AgentToolGroup<T>[];
};

const DEFAULT_AGENT_TOOL_NAME_FALLBACKS = new Set([
  '最终回复',
  'final_response',
  '定时任务',
  'schedule_task',
  '休眠等待',
  'sleep',
  'sleep_wait',
  '记忆管理',
  'memory_manager',
  '执行命令',
  'execute_command',
  'ptc',
  'programmatic_tool_call',
  '列出文件',
  'list_files',
  '搜索内容',
  'search_content',
  '读取文件',
  'read_file',
  '技能调用',
  'skill_call',
  '写入文件',
  'write_file',
  '应用补丁',
  'apply_patch',
  '技能创建器'
]);

const normalizeNameList = (list: unknown): string[] => {
  if (!Array.isArray(list)) return [];
  const seen = new Set<string>();
  const output: string[] = [];
  for (const item of list) {
    const name = String(item || '').trim();
    if (!name || seen.has(name)) continue;
    seen.add(name);
    output.push(name);
  }
  return output;
};

const normalizeToolList = <T extends ToolOptionLike>(
  list: unknown,
  normalizeOption: (item: unknown) => T | null
): T[] => {
  if (!Array.isArray(list)) return [];
  return list.map((item) => normalizeOption(item)).filter(Boolean) as T[];
};

const buildGroup = <T extends ToolOptionLike>(
  key: string,
  label: string,
  list: unknown,
  normalizeOption: (item: unknown) => T | null
): AgentToolGroup<T> => ({
  key,
  label,
  options: normalizeToolList(list, normalizeOption)
});

export const buildAgentToolSections = <T extends ToolOptionLike>(
  payload: Record<string, unknown> | null | undefined,
  t: (key: string) => string,
  normalizeOption: (item: unknown) => T | null
): AgentToolSection<T>[] => {
  const source = payload || {};
  const adminGroups = [
    buildGroup('builtin', t('portal.agent.tools.group.builtin'), source.admin_builtin_tools ?? source.builtin_tools, normalizeOption),
    buildGroup('mcp', t('portal.agent.tools.group.mcp'), source.admin_mcp_tools ?? source.mcp_tools, normalizeOption),
    buildGroup('a2a', t('portal.agent.tools.group.a2a'), source.admin_a2a_tools ?? source.a2a_tools, normalizeOption),
    buildGroup('skills', t('portal.agent.tools.group.skills'), source.admin_skills ?? source.skills, normalizeOption),
    buildGroup('knowledge', t('portal.agent.tools.group.knowledge'), source.admin_knowledge_tools ?? source.knowledge_tools, normalizeOption)
  ].filter((group) => group.options.length > 0);

  const userGroups = [
    buildGroup('user-mcp', t('portal.agent.tools.group.mcp'), source.user_mcp_tools, normalizeOption),
    buildGroup('user-skills', t('portal.agent.tools.group.skills'), source.user_skills, normalizeOption),
    buildGroup('user-knowledge', t('portal.agent.tools.group.knowledge'), source.user_knowledge_tools, normalizeOption)
  ].filter((group) => group.options.length > 0);

  if (!userGroups.length) {
    const legacyUserGroup = buildGroup(
      'user',
      t('portal.agent.tools.group.user'),
      source.user_tools,
      normalizeOption
    );
    if (legacyUserGroup.options.length) {
      userGroups.push(legacyUserGroup);
    }
  }

  return [
    { key: 'admin', label: t('portal.agent.tools.section.admin'), groups: adminGroups },
    { key: 'user', label: t('portal.agent.tools.section.user'), groups: userGroups }
  ].filter((section) => section.groups.length > 0);
};

export const collectToolValuesFromSections = <T extends ToolOptionLike>(
  sections: AgentToolSection<T>[]
): string[] => {
  const seen = new Set<string>();
  const output: string[] = [];
  sections.forEach((section) => {
    section.groups.forEach((group) => {
      group.options.forEach((option) => {
        if (seen.has(option.value)) return;
        seen.add(option.value);
        output.push(option.value);
      });
    });
  });
  return output;
};

export const resolveDefaultAgentToolNames = <T extends ToolOptionLike>(
  payload: Record<string, unknown> | null | undefined,
  sections: AgentToolSection<T>[]
): string[] => {
  const available = new Set(collectToolValuesFromSections(sections));
  const configured = normalizeNameList(payload?.default_agent_tool_names).filter((name) =>
    available.has(name)
  );
  if (configured.length) {
    return configured;
  }
  return Array.from(available).filter((name) => DEFAULT_AGENT_TOOL_NAME_FALLBACKS.has(name));
};
