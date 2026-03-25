export type AgentAvatarImageOption = {
  key: string;
  image: string;
  label: string;
};

const AGENT_AVATAR_IMAGE_FILES = import.meta.glob('../assets/agent-avatars/avatar-???.jpg', {
  eager: true,
  import: 'default'
}) as Record<string, string>;

export const AGENT_AVATAR_IMAGE_OPTIONS: AgentAvatarImageOption[] = Object.entries(
  AGENT_AVATAR_IMAGE_FILES
)
  .map(([path, image]) => {
    const fileName = path.split('/').pop() || '';
    const stem = fileName.replace(/\.jpg$/i, '').trim();
    const numericPart = stem.replace(/^avatar-/, '').trim();
    const sequence = Number.parseInt(numericPart, 10);
    const label = Number.isFinite(sequence)
      ? `Agent Avatar ${String(sequence).padStart(3, '0')}`
      : `Agent Avatar ${stem}`;
    return {
      key: stem,
      image,
      label
    };
  })
  .sort((left, right) => left.key.localeCompare(right.key, 'en', { numeric: true, sensitivity: 'base' }));

export const AGENT_AVATAR_IMAGE_MAP = new Map(
  AGENT_AVATAR_IMAGE_OPTIONS.map((item) => [item.key, item.image])
);

export const AGENT_AVATAR_OPTION_KEYS = new Set<string>([
  'initial',
  ...AGENT_AVATAR_IMAGE_OPTIONS.map((item) => item.key)
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

export const DEFAULT_AGENT_AVATAR_IMAGE_KEY = AGENT_AVATAR_IMAGE_MAP.has('avatar-000')
  ? 'avatar-000'
  : AGENT_AVATAR_IMAGE_OPTIONS[0]?.key || 'initial';

export const resolveAgentAvatarImageByKey = (key: unknown): string =>
  AGENT_AVATAR_IMAGE_MAP.get(String(key || '').trim()) || '';
