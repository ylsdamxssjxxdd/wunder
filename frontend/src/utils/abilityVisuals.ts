export type AbilityVisualTone =
  | 'general'
  | 'skill'
  | 'mcp'
  | 'knowledge'
  | 'shared'
  | 'automation'
  | 'search'
  | 'file'
  | 'terminal';

export type AbilityVisualInput = {
  name?: unknown;
  description?: unknown;
  hint?: unknown;
  kind?: unknown;
  group?: unknown;
  source?: unknown;
};

export type AbilityVisualMeta = {
  icon: string;
  tone: AbilityVisualTone;
};

type AbilityKind = 'tool' | 'skill';

type AbilityRule = {
  keywords: string[];
  icon: string;
  tone: AbilityVisualTone;
};

const ABILITY_RULES: AbilityRule[] = [
  { keywords: ['用户世界工具', 'user_world', 'user world'], icon: 'fa-earth-asia', tone: 'general' },
  { keywords: ['会话让出', 'sessions_yield', 'session yield', 'yield'], icon: 'fa-share-from-square', tone: 'automation' },
  { keywords: ['自我状态', 'self_status', 'self status'], icon: 'fa-gauge-high', tone: 'general' },
  { keywords: ['桌面控制器'], icon: 'fa-computer-mouse', tone: 'general' },
  { keywords: ['桌面监视器'], icon: 'fa-display', tone: 'general' },
  { keywords: ['读图工具', 'view_image', 'view image'], icon: 'fa-image', tone: 'search' },
  { keywords: ['渠道工具', 'channel_tool', 'channel tool', 'channel_send', 'channel_contacts'], icon: 'fa-comments', tone: 'general' },
  { keywords: ['LSP查询', 'lsp query', 'lsp'], icon: 'fa-code', tone: 'file' },
  {
    keywords: ['最终回复', 'final response', 'final answer', 'final reply', 'final_response'],
    icon: 'fa-paper-plane',
    tone: 'general'
  },
  { keywords: ['desktop_controller', 'desktop controller'], icon: 'fa-computer-mouse', tone: 'general' },
  { keywords: ['desktop_monitor', 'desktop monitor', '桌面监控'], icon: 'fa-display', tone: 'general' },
  { keywords: ['update_plan', 'plan board', '计划面板', '计划看板'], icon: 'fa-table-columns', tone: 'automation' },
  { keywords: ['question_panel', 'ask_panel', 'question panel', '问询面板'], icon: 'fa-circle-question', tone: 'general' },
  {
    keywords: ['browser_navigate', 'browser_click', 'browser_type', 'browser_screenshot', 'browser_read_page'],
    icon: 'fa-window-maximize',
    tone: 'search'
  },
  { keywords: ['browser', '浏览器'], icon: 'fa-window-maximize', tone: 'search' },
  { keywords: ['a2a_observe', 'a2a observe', 'a2a观察'], icon: 'fa-glasses', tone: 'automation' },
  { keywords: ['a2a_wait', 'a2a wait', 'a2a等待'], icon: 'fa-clock', tone: 'automation' },
  { keywords: ['a2ui'], icon: 'fa-image', tone: 'search' },
  { keywords: ['agent_swarm', 'swarm_control', '智能体蜂群'], icon: 'fa-bee', tone: 'automation' },
  { keywords: ['subagent_control', '子智能体控制'], icon: 'fa-diagram-project', tone: 'automation' },
  {
    keywords: ['node.invoke', 'node_invoke', 'node invoke', 'gateway_invoke', 'gateway invoke'],
    icon: 'fa-diagram-project',
    tone: 'automation'
  },
  { keywords: ['thread_control', 'session_thread', '会话线程控制'], icon: 'fa-code-branch', tone: 'automation' },
  { keywords: ['skill_call', 'skill_get', '技能调用'], icon: 'fa-book-open', tone: 'skill' },
  { keywords: ['cron', 'schedule_task', 'scheduled task', 'timer'], icon: 'fa-clock', tone: 'automation' },
  { keywords: ['计划任务', '定时任务'], icon: 'fa-clock', tone: 'automation' },
  { keywords: ['sleep_wait', 'sleep', 'pause'], icon: 'fa-hourglass-half', tone: 'automation' },
  { keywords: ['休眠等待'], icon: 'fa-hourglass-half', tone: 'automation' },
  { keywords: ['memory_manager', 'memory_manage', 'memory manager', 'memory'], icon: 'fa-memory', tone: 'automation' },
  { keywords: ['记忆管理'], icon: 'fa-memory', tone: 'automation' },
  { keywords: ['thread_control', 'session_thread', 'thread'], icon: 'fa-code-branch', tone: 'automation' },
  {
    keywords: ['subagent_control', 'node.invoke', 'node_invoke', 'gateway_invoke', 'a2a', 'subagent', 'swarm'],
    icon: 'fa-diagram-project',
    tone: 'automation'
  },
  { keywords: ['web_fetch', 'web fetch', 'webfetch', 'browse'], icon: 'fa-globe', tone: 'search' },
  { keywords: ['网页抓取'], icon: 'fa-globe', tone: 'search' },
  { keywords: ['list_files', 'list_file', 'list files'], icon: 'fa-folder-open', tone: 'file' },
  { keywords: ['列出文件'], icon: 'fa-folder-open', tone: 'file' },
  { keywords: ['read_image', 'read image'], icon: 'fa-image', tone: 'search' },
  { keywords: ['search_content', 'search content'], icon: 'fa-magnifying-glass', tone: 'search' },
  { keywords: ['搜索内容', '搜索', '检索'], icon: 'fa-magnifying-glass', tone: 'search' },
  { keywords: ['read_file', 'read file'], icon: 'fa-file-lines', tone: 'file' },
  { keywords: ['读取文件'], icon: 'fa-file-lines', tone: 'file' },
  { keywords: ['write_file', 'write file'], icon: 'fa-file-circle-plus', tone: 'file' },
  { keywords: ['写入文件'], icon: 'fa-file-circle-plus', tone: 'file' },
  { keywords: ['apply_patch', 'apply patch'], icon: 'fa-pen-to-square', tone: 'file' },
  { keywords: ['应用补丁'], icon: 'fa-pen-to-square', tone: 'file' },
  { keywords: ['programmatic_tool_call', 'ptc'], icon: 'fa-code', tone: 'file' },
  { keywords: ['write_file', 'write file'], icon: 'fa-file-circle-plus', tone: 'file' },
  {
    keywords: ['skill', 'skills', 'prompt', 'workflow', 'template', 'agent preset', 'preset'],
    icon: 'fa-wand-magic-sparkles',
    tone: 'skill'
  },
  { keywords: ['knowledge', 'rag', 'vector', 'embedding', 'document', 'kb'], icon: 'fa-book', tone: 'knowledge' },
  { keywords: ['知识'], icon: 'fa-book', tone: 'knowledge' },
  { keywords: ['mcp', 'connector', 'integration', 'endpoint'], icon: 'fa-plug', tone: 'mcp' },
  { keywords: ['shared', 'share'], icon: 'fa-wrench', tone: 'shared' },
  { keywords: ['search', 'query', 'retrieve'], icon: 'fa-magnifying-glass', tone: 'search' },
  {
    keywords: [
      'shell',
      'terminal',
      'command',
      'powershell',
      'bash',
      'cmd',
      'execute_command',
      'run command',
      'execute command',
      '执行命令',
      '运行命令'
    ],
    icon: 'fa-terminal',
    tone: 'terminal'
  },
  { keywords: ['file', 'files', 'read', 'write', 'patch', 'edit', 'folder', 'workspace'], icon: 'fa-file-lines', tone: 'file' },
  { keywords: ['image', 'vision', 'camera', 'screenshot'], icon: 'fa-image', tone: 'search' }
];

const cleanText = (value: unknown): string => String(value || '').trim();

const normalizeMatchKey = (value: string): string =>
  value
    .trim()
    .toLowerCase()
    .replace(/[\s_.\-:/\\@]+/g, '');

export const isAbilitySkillGroup = (value: unknown): boolean =>
  cleanText(value).toLowerCase().includes('skill');

export const resolveAbilityKind = (value: unknown, group: unknown = ''): AbilityKind => {
  const normalized = cleanText(value).toLowerCase();
  if (normalized === 'skill') {
    return 'skill';
  }
  return isAbilitySkillGroup(group) ? 'skill' : 'tool';
};

export const resolveAbilitySummary = (description: unknown, hint: unknown): string => {
  const detail = cleanText(description);
  if (detail) {
    return detail;
  }
  return cleanText(hint);
};

const resolvePreferredTone = (kind: AbilityKind, group: unknown, source: unknown): AbilityVisualTone | '' => {
  const grouped = cleanText(group).toLowerCase();
  const sourced = cleanText(source).toLowerCase();
  const key = `${grouped}:${sourced}`;
  if (grouped === 'knowledge' || sourced === 'knowledge' || sourced === 'user_knowledge' || key.includes('knowledge')) {
    return 'knowledge';
  }
  if (grouped === 'mcp' || sourced === 'mcp' || sourced === 'user_mcp' || key.includes('mcp')) {
    return 'mcp';
  }
  if (grouped === 'shared' || sourced === 'shared') {
    return 'shared';
  }
  if (grouped === 'a2a' || sourced === 'a2a' || key.includes('a2a')) {
    return 'automation';
  }
  if (kind === 'skill') {
    return 'skill';
  }
  return '';
};

const resolveDefaultIcon = (tone: AbilityVisualTone): string => {
  switch (tone) {
    case 'skill':
      return 'fa-wand-magic-sparkles';
    case 'mcp':
      return 'fa-plug';
    case 'knowledge':
      return 'fa-book';
    case 'shared':
      return 'fa-wrench';
    case 'automation':
      return 'fa-diagram-project';
    case 'search':
      return 'fa-magnifying-glass';
    case 'file':
      return 'fa-file-lines';
    case 'terminal':
      return 'fa-terminal';
    default:
      return 'fa-toolbox';
  }
};

const resolveContextualDefaultIcon = (input: AbilityVisualInput, tone: AbilityVisualTone): string => {
  const name = cleanText(input.name);
  const grouped = cleanText(input.group).toLowerCase();
  const sourced = cleanText(input.source).toLowerCase();
  if (tone !== 'mcp' && tone !== 'knowledge' && tone !== 'skill') {
    if (name.includes('@')) {
      return 'fa-wrench';
    }
    if (
      grouped === 'user' ||
      sourced === 'user' ||
      grouped.startsWith('user-') ||
      sourced.startsWith('user-')
    ) {
      return 'fa-wrench';
    }
  }
  return resolveDefaultIcon(tone);
};

const findAbilityRule = (input: AbilityVisualInput): AbilityRule | null => {
  const text = [
    cleanText(input.name),
    cleanText(input.description),
    cleanText(input.hint),
    cleanText(input.group),
    cleanText(input.source)
  ]
    .filter(Boolean)
    .join(' ');

  if (!text) {
    return null;
  }

  const lowerText = text.toLowerCase();
  const normalizedText = normalizeMatchKey(text);

  for (const rule of ABILITY_RULES) {
    for (const keyword of rule.keywords) {
      const lowerKeyword = keyword.toLowerCase();
      if (lowerText.includes(lowerKeyword)) {
        return rule;
      }
      const normalizedKeyword = normalizeMatchKey(lowerKeyword);
      if (normalizedKeyword && normalizedText.includes(normalizedKeyword)) {
        return rule;
      }
    }
  }

  return null;
};

export const resolveAbilityVisual = (input: AbilityVisualInput): AbilityVisualMeta => {
  const kind = resolveAbilityKind(input.kind, input.group || input.source);
  const preferredTone = resolvePreferredTone(kind, input.group, input.source);
  if (kind === 'skill') {
    return {
      icon: resolveDefaultIcon('skill'),
      tone: preferredTone || 'skill'
    };
  }
  const matchedRule = findAbilityRule(input);
  const tone = preferredTone || matchedRule?.tone || 'general';

  return {
    icon: matchedRule?.icon || resolveContextualDefaultIcon(input, tone),
    tone
  };
};

export const resolveAbilityPitchKey = (input: AbilityVisualInput): string => {
  const visual = resolveAbilityVisual(input);
  switch (visual.tone) {
    case 'skill':
      return 'chat.ability.pitch.skill';
    case 'mcp':
      return 'chat.ability.pitch.mcp';
    case 'knowledge':
      return 'chat.ability.pitch.knowledge';
    case 'shared':
      return 'chat.ability.pitch.shared';
    default:
      return 'chat.ability.pitch.tool';
  }
};
