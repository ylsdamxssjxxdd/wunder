import { getDesktopRuntime } from '@/config/desktop';
import { getDemoToken, isDemoMode } from '@/utils/demo';
import { readAccessToken } from '@/utils/authTokenStorage';

export const resolveAccessToken = (): string => {
  if (isDemoMode()) {
    return getDemoToken();
  }

  const storedToken = readAccessToken();
  if (storedToken) {
    return storedToken;
  }

  return String(getDesktopRuntime()?.token || '').trim();
};
