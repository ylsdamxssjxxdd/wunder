import { getDesktopRuntime, isDesktopRemoteAuthMode } from '@/config/desktop';
import { getDemoToken, isDemoMode } from '@/utils/demo';

const readStoredAccessToken = (): string => {
  try {
    return String(localStorage.getItem('access_token') || '').trim();
  } catch {
    return '';
  }
};

export const resolveAccessToken = (): string => {
  if (isDemoMode()) {
    return getDemoToken();
  }

  const storedToken = readStoredAccessToken();
  if (storedToken) {
    return storedToken;
  }

  if (isDesktopRemoteAuthMode()) {
    return '';
  }

  return String(getDesktopRuntime()?.token || '').trim();
};
