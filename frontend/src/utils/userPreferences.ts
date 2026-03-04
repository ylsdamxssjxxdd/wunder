export const USER_APPEARANCE_STORAGE_PREFIX = 'messenger_user_appearance_v1:';
const LEGACY_USER_AVATAR_STORAGE_PREFIX = 'messenger_user_avatar_v1:';
const LEGACY_THEME_MODE_STORAGE_KEY = 'wille-user-theme';
const LEGACY_THEME_PALETTE_STORAGE_KEY = 'wille-user-accent-theme';

export const DEFAULT_THEME_MODE = 'light';
export const DEFAULT_THEME_PALETTE = 'eva-orange';
export const DEFAULT_AVATAR_ICON = 'initial';
export const DEFAULT_AVATAR_COLOR = '#3b82f6';

export type ThemeMode = 'dark' | 'light';
export type ThemePalette = 'hula-green' | 'eva-orange' | 'minimal';

export type UserAppearancePreferences = {
  themeMode: ThemeMode;
  themePalette: ThemePalette;
  avatarIcon: string;
  avatarColor: string;
  updatedAt: number;
};

const LEGACY_AVATAR_ALIAS_MAP: Record<string, string> = {
  initial: 'initial',
  check: 'initial',
  spark: 'initial',
  target: 'initial',
  idea: 'initial',
  code: 'initial',
  pen: 'initial',
  briefcase: 'initial',
  shield: 'initial',
  'fa-user': 'initial',
  'fa-user-astronaut': 'initial',
  'fa-rocket': 'initial',
  'fa-lightbulb': 'initial',
  'fa-code': 'initial',
  'fa-pen': 'initial',
  'fa-briefcase': 'initial',
  'fa-shield-halved': 'initial'
};

export const normalizeThemeMode = (value: unknown): ThemeMode => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  return text === 'dark' ? 'dark' : DEFAULT_THEME_MODE;
};

export const normalizeThemePalette = (value: unknown): ThemePalette => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'hula-green' || text === 'minimal') {
    return text as ThemePalette;
  }
  return DEFAULT_THEME_PALETTE;
};

export const normalizeAvatarIcon = (value: unknown, allowedKeys?: Set<string>): string => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (!text) return DEFAULT_AVATAR_ICON;
  const alias = LEGACY_AVATAR_ALIAS_MAP[text];
  const normalized = alias || text;
  const legacyMatch = normalized.match(/^qq-avatar-(\d{1,4})$/);
  const upgraded = legacyMatch
    ? `qq-avatar-${String(Number.parseInt(legacyMatch[1], 10)).padStart(4, '0')}`
    : normalized;
  if (allowedKeys && !allowedKeys.has(upgraded)) {
    return DEFAULT_AVATAR_ICON;
  }
  return upgraded;
};

export const normalizeAvatarColor = (value: unknown): string => {
  const text = String(value || '').trim();
  if (/^#[0-9a-fA-F]{6}$/.test(text)) return text.toLowerCase();
  return DEFAULT_AVATAR_COLOR;
};

export const resolveAppearanceStorageKey = (userId: unknown): string => {
  const cleaned = String(userId || '').trim() || 'guest';
  return `${USER_APPEARANCE_STORAGE_PREFIX}${cleaned}`;
};

export const defaultUserAppearance = (): UserAppearancePreferences => ({
  themeMode: DEFAULT_THEME_MODE,
  themePalette: DEFAULT_THEME_PALETTE,
  avatarIcon: DEFAULT_AVATAR_ICON,
  avatarColor: DEFAULT_AVATAR_COLOR,
  updatedAt: 0
});

export const normalizeUserAppearance = (
  payload: unknown,
  allowedAvatarKeys?: Set<string>
): UserAppearancePreferences => {
  const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
  const next = defaultUserAppearance();
  next.themeMode = normalizeThemeMode(source.theme_mode ?? source.themeMode);
  next.themePalette = normalizeThemePalette(source.theme_palette ?? source.themePalette);
  next.avatarIcon = normalizeAvatarIcon(source.avatar_icon ?? source.avatarIcon, allowedAvatarKeys);
  next.avatarColor = normalizeAvatarColor(source.avatar_color ?? source.avatarColor);
  const updatedAt = Number(source.updated_at ?? source.updatedAt ?? 0);
  next.updatedAt = Number.isFinite(updatedAt) && updatedAt > 0 ? updatedAt : 0;
  return next;
};

export const readUserAppearanceFromStorage = (
  userId: unknown,
  allowedAvatarKeys?: Set<string>
): UserAppearancePreferences => {
  if (typeof window === 'undefined') {
    return defaultUserAppearance();
  }
  try {
    const raw = window.localStorage.getItem(resolveAppearanceStorageKey(userId));
    if (!raw) {
      return readLegacyUserAppearanceFromStorage(userId, allowedAvatarKeys);
    }
    return normalizeUserAppearance(JSON.parse(raw), allowedAvatarKeys);
  } catch {
    return readLegacyUserAppearanceFromStorage(userId, allowedAvatarKeys);
  }
};

export const writeUserAppearanceToStorage = (
  userId: unknown,
  value: UserAppearancePreferences
): void => {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(resolveAppearanceStorageKey(userId), JSON.stringify(value));
    window.localStorage.setItem(LEGACY_THEME_MODE_STORAGE_KEY, value.themeMode);
    window.localStorage.setItem(LEGACY_THEME_PALETTE_STORAGE_KEY, value.themePalette);
    const legacyAvatarKey = `${LEGACY_USER_AVATAR_STORAGE_PREFIX}${String(userId || '').trim() || 'guest'}`;
    window.localStorage.setItem(
      legacyAvatarKey,
      JSON.stringify({
        icon: value.avatarIcon,
        color: value.avatarColor
      })
    );
  } catch {
    // ignore localStorage errors
  }
};

const readLegacyUserAppearanceFromStorage = (
  userId: unknown,
  allowedAvatarKeys?: Set<string>
): UserAppearancePreferences => {
  const next = defaultUserAppearance();
  if (typeof window === 'undefined') {
    return next;
  }
  next.themeMode = normalizeThemeMode(window.localStorage.getItem(LEGACY_THEME_MODE_STORAGE_KEY));
  next.themePalette = normalizeThemePalette(window.localStorage.getItem(LEGACY_THEME_PALETTE_STORAGE_KEY));
  const legacyAvatarKey = `${LEGACY_USER_AVATAR_STORAGE_PREFIX}${String(userId || '').trim() || 'guest'}`;
  try {
    const raw = window.localStorage.getItem(legacyAvatarKey);
    if (raw) {
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      next.avatarIcon = normalizeAvatarIcon(parsed.icon, allowedAvatarKeys);
      next.avatarColor = normalizeAvatarColor(parsed.color);
    }
  } catch {
    next.avatarIcon = DEFAULT_AVATAR_ICON;
    next.avatarColor = DEFAULT_AVATAR_COLOR;
  }
  return next;
};
