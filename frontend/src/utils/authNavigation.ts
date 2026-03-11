import type { LocationQuery } from 'vue-router';

export const FORCE_LOGOUT_QUERY_KEY = 'force_logout';
export const FORCE_LOGOUT_QUERY_VALUE = '1';
export const FORCE_LOGOUT_LOGIN_PATH = `/login?${FORCE_LOGOUT_QUERY_KEY}=${FORCE_LOGOUT_QUERY_VALUE}`;

const asQueryText = (value: unknown): string => {
  if (Array.isArray(value)) {
    for (const item of value) {
      const text = String(item || '').trim();
      if (text) return text;
    }
    return '';
  }
  return String(value || '').trim();
};

export const isForcedLogoutQuery = (query: LocationQuery | Record<string, unknown>): boolean =>
  asQueryText((query as Record<string, unknown>)[FORCE_LOGOUT_QUERY_KEY]) === FORCE_LOGOUT_QUERY_VALUE;

export const redirectToLoginAfterLogout = (
  replace?: (to: string) => Promise<unknown> | unknown
): void => {
  const target = FORCE_LOGOUT_LOGIN_PATH;
  if (replace) {
    Promise.resolve(replace(target)).catch(() => {
      if (typeof window !== 'undefined') {
        window.location.replace(target);
      }
    });
    return;
  }
  if (typeof window !== 'undefined') {
    window.location.replace(target);
  }
};
