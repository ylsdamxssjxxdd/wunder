import { fetchMyPreferences, updateMyPreferences } from '@/api/auth';
import {
  defaultUserAppearance,
  normalizeUserAppearance,
  readUserAppearanceFromStorage,
  writeUserAppearanceToStorage,
  type UserAppearancePreferences
} from '@/utils/userPreferences';

const nowSeconds = () => Date.now() / 1000;

const pickLatestAppearance = (
  localAppearance: UserAppearancePreferences,
  remoteAppearance: UserAppearancePreferences
): UserAppearancePreferences =>
  remoteAppearance.updatedAt > localAppearance.updatedAt ? remoteAppearance : localAppearance;

const isSameAppearance = (
  left: UserAppearancePreferences,
  right: UserAppearancePreferences
): boolean =>
  left.themeMode === right.themeMode &&
  left.themePalette === right.themePalette &&
  left.avatarIcon === right.avatarIcon &&
  left.avatarColor === right.avatarColor;

const toRemotePayload = (appearance: UserAppearancePreferences) => ({
  theme_mode: appearance.themeMode,
  theme_palette: appearance.themePalette,
  avatar_icon: appearance.avatarIcon,
  avatar_color: appearance.avatarColor
});

export const loadUserAppearance = async (
  userId: string,
  allowedAvatarKeys?: Set<string>
): Promise<UserAppearancePreferences> => {
  const cleanedUserId = String(userId || '').trim();
  if (!cleanedUserId) {
    return defaultUserAppearance();
  }
  const localAppearance = readUserAppearanceFromStorage(cleanedUserId, allowedAvatarKeys);
  try {
    const { data } = await fetchMyPreferences();
    const remoteAppearance = normalizeUserAppearance(data?.data, allowedAvatarKeys);
    const latest = pickLatestAppearance(localAppearance, remoteAppearance);
    writeUserAppearanceToStorage(cleanedUserId, latest);
    if (latest === localAppearance && !isSameAppearance(localAppearance, remoteAppearance)) {
      void saveUserAppearance(cleanedUserId, localAppearance, allowedAvatarKeys);
    }
    return latest;
  } catch {
    writeUserAppearanceToStorage(cleanedUserId, localAppearance);
    return localAppearance;
  }
};

export const saveUserAppearance = async (
  userId: string,
  appearance: UserAppearancePreferences,
  allowedAvatarKeys?: Set<string>
): Promise<UserAppearancePreferences> => {
  const cleanedUserId = String(userId || '').trim();
  if (!cleanedUserId) {
    return defaultUserAppearance();
  }
  const normalized = normalizeUserAppearance(
    {
      theme_mode: appearance.themeMode,
      theme_palette: appearance.themePalette,
      avatar_icon: appearance.avatarIcon,
      avatar_color: appearance.avatarColor,
      updated_at: nowSeconds()
    },
    allowedAvatarKeys
  );
  writeUserAppearanceToStorage(cleanedUserId, normalized);
  try {
    const { data } = await updateMyPreferences(toRemotePayload(normalized));
    const remoteNormalized = normalizeUserAppearance(data?.data, allowedAvatarKeys);
    const output =
      remoteNormalized.updatedAt > 0
        ? remoteNormalized
        : {
            ...remoteNormalized,
            updatedAt: normalized.updatedAt
          };
    writeUserAppearanceToStorage(cleanedUserId, output);
    return output;
  } catch {
    return normalized;
  }
};
