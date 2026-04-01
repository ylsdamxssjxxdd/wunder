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
  { keywords: ['final response', 'final answer', 'final_response'], icon: 'fa-flag-checkered', tone: 'general' },
  { keywords: ['cron', 'schedule_task', 'scheduled task', 'timer'], icon: 'fa-clock', tone: 'automation' },
  { keywords: ['sleep_wait', 'sleep', 'pause'], icon: 'fa-hourglass-half', tone: 'automation' },
  { keywords: ['memory_manager', 'memory_manage', 'memory manager', 'memory'], icon: 'fa-memory', tone: 'automation' },
  { keywords: ['thread_control', 'session_thread', 'thread'], icon: 'fa-code-branch', tone: 'automation' },
  {
    keywords: ['subagent_control', 'node.invoke', 'node_invoke', 'gateway_invoke', 'a2a', 'subagent', 'swarm'],
    icon: 'fa-diagram-project',
    tone: 'automation'
  },
  { keywords: ['desktop_controller', 'desktop controller'], icon: 'fa-computer-mouse', tone: 'general' },
  { keywords: ['desktop_monitor', 'desktop monitor'], icon: 'fa-display', tone: 'general' },
  { keywords: ['web_fetch', 'web fetch', 'webfetch', 'browse'], icon: 'fa-globe', tone: 'search' },
  { keywords: ['list_files', 'list_file', 'list files'], icon: 'fa-folder-open', tone: 'file' },
  { keywords: ['write_file', 'write file'], icon: 'fa-file-circle-plus', tone: 'file' },
  { keywords: ['apply_patch', 'apply patch'], icon: 'fa-pen-to-square', tone: 'file' },
  { keywords: ['programmatic_tool_call', 'ptc'], icon: 'fa-code', tone: 'file' },
  {
    keywords: ['skill', 'skills', 'prompt', 'workflow', 'template', 'agent preset', 'preset'],
    icon: 'fa-wand-magic-sparkles',
    tone: 'skill'
  },
  { keywords: ['knowledge', 'rag', 'vector', 'embedding', 'document', 'kb'], icon: 'fa-book', tone: 'knowledge' },
  { keywords: ['mcp', 'connector', 'integration', 'endpoint', 'service', 'server'], icon: 'fa-plug', tone: 'mcp' },
  { keywords: ['shared', 'share'], icon: 'fa-share-nodes', tone: 'shared' },
  { keywords: ['search', 'query', 'retrieve', 'web'], icon: 'fa-magnifying-glass', tone: 'search' },
  { keywords: ['shell', 'terminal', 'command', 'powershell', 'bash', 'cmd'], icon: 'fa-terminal', tone: 'terminal' },
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
      return 'fa-share-nodes';
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
  const matchedRule = findAbilityRule(input);
  const tone = preferredTone || matchedRule?.tone || (kind === 'skill' ? 'skill' : 'general');

  return {
    icon: matchedRule?.icon || resolveDefaultIcon(tone),
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
