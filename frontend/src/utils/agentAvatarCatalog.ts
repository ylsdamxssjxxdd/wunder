const AGENT_AVATAR_IMAGE_FILES = import.meta.glob(
  [
    '../assets/agent-avatars/avatar-???.jpg',
    '../assets/agent-avatars/avatar-???.jpeg',
    '../assets/agent-avatars/avatar-???.png'
  ],
  {
    eager: true,
    import: 'default'
  }
) as Record<string, string>;

const AGENT_AVATAR_IMAGE_EXTENSION_PRIORITY: Record<string, number> = {
  png: 3,
  jpg: 2,
  jpeg: 1
};

const agentAvatarImageEntryByKey = new Map<
  string,
  {
    key: string;
    image: string;
    priority: number;
  }
>();

Object.entries(AGENT_AVATAR_IMAGE_FILES).forEach(([path, image]) => {
  const fileName = path.split('/').pop() || '';
  const extension = (fileName.split('.').pop() || '').trim().toLowerCase();
  const stem = fileName.replace(/\.(?:jpe?g|png)$/i, '').trim();
  if (!stem) {
    return;
  }
  const priority = AGENT_AVATAR_IMAGE_EXTENSION_PRIORITY[extension] || 0;
  const current = agentAvatarImageEntryByKey.get(stem);
  if (current && current.priority >= priority) {
    return;
  }
  agentAvatarImageEntryByKey.set(stem, {
    key: stem,
    image,
    priority
  });
});

const AGENT_AVATAR_IMAGE_ENTRIES = Array.from(agentAvatarImageEntryByKey.values())
  .map(({ key, image }) => ({
    key,
    image
  }))
  .sort((left, right) => left.key.localeCompare(right.key, 'en', { numeric: true, sensitivity: 'base' }));

export const AGENT_AVATAR_IMAGE_KEYS = AGENT_AVATAR_IMAGE_ENTRIES.map((item) => item.key);

export const AGENT_AVATAR_IMAGE_MAP = new Map(
  AGENT_AVATAR_IMAGE_ENTRIES.map((item) => [item.key, item.image])
);

export const AGENT_AVATAR_OPTION_KEYS = new Set<string>([
  'initial',
  ...AGENT_AVATAR_IMAGE_KEYS
]);

export const AGENT_AVATAR_COLORS = [
  '#f97316',
  '#ef4444',
  '#ec4899',
  '#8b5cf6',
  '#6366f1',
  '#3b82f6',
  '#06b6d4',
  '#14b8a6',
  '#10b981',
  '#84cc16',
  '#f59e0b',
  '#64748b'
] as const;

export const DEFAULT_AGENT_AVATAR_IMAGE_KEY = AGENT_AVATAR_IMAGE_MAP.has('avatar-046')
  ? 'avatar-046'
  : AGENT_AVATAR_IMAGE_KEYS[0] || 'initial';

export const resolveAgentAvatarImageByKey = (key: unknown): string =>
  AGENT_AVATAR_IMAGE_MAP.get(String(key || '').trim()) || '';
