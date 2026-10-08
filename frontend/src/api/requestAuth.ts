import { getDemoToken, isDemoMode } from '@/utils/demo';
import { readAccessToken } from '@/utils/authTokenStorage';

export const resolveAccessToken = (): string => {
  if (isDemoMode()) {
    return getDemoToken();
  }

  const storedToken = readAccessToken();
  return storedToken ? storedToken : '';
};
