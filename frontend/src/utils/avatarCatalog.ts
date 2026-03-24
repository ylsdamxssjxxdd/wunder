export type ProfileAvatarImageOption = {
  key: string;
  image: string;
  label: string;
};

const PROFILE_AVATAR_IMAGE_FILES = import.meta.glob('../assets/qq-avatars/avatar-????.jpg', {
  eager: true,
  import: 'default'
}) as Record<string, string>;

export const PROFILE_AVATAR_IMAGE_OPTIONS: ProfileAvatarImageOption[] = Object.entries(
  PROFILE_AVATAR_IMAGE_FILES
)
  .map(([path, image]) => {
    const fileName = path.split('/').pop() || '';
    const stem = fileName.replace(/\.jpg$/i, '').trim();
    const numericPart = stem.replace(/^avatar-/, '').trim();
    const sequence = Number.parseInt(numericPart, 10);
    const label = Number.isFinite(sequence)
      ? `QQ Avatar ${String(sequence).padStart(4, '0')}`
      : `QQ Avatar ${stem}`;
    return {
      key: `qq-${stem}`,
      image,
      label
    };
  })
  .sort((left, right) => left.key.localeCompare(right.key, 'en', { numeric: true, sensitivity: 'base' }));

export const PROFILE_AVATAR_IMAGE_MAP = new Map(
  PROFILE_AVATAR_IMAGE_OPTIONS.map((item) => [item.key, item.image])
);

export const PROFILE_AVATAR_OPTION_KEYS = new Set<string>([
  'initial',
  ...PROFILE_AVATAR_IMAGE_OPTIONS.map((item) => item.key)
]);

export const PROFILE_AVATAR_COLORS = [
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

export const DEFAULT_PROFILE_AVATAR_IMAGE_KEY = PROFILE_AVATAR_IMAGE_MAP.has('qq-avatar-0119')
  ? 'qq-avatar-0119'
  : PROFILE_AVATAR_IMAGE_OPTIONS[0]?.key || 'initial';

export const resolveProfileAvatarImageByKey = (key: unknown): string =>
  PROFILE_AVATAR_IMAGE_MAP.get(String(key || '').trim()) || '';

