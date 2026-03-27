import { isNavigationFailure, type LocationQuery, type LocationQueryRaw, type RouteLocationRaw } from 'vue-router';

export const FORCE_LOGOUT_QUERY_KEY = 'force_logout';
export const FORCE_LOGOUT_QUERY_VALUE = '1';
export const FORCE_LOGOUT_LOGIN_PATH = `/login?${FORCE_LOGOUT_QUERY_KEY}=${FORCE_LOGOUT_QUERY_VALUE}`;
export const DEFAULT_AGENT_CHAT_QUERY = Object.freeze({
  section: 'messages',
  entry: 'default'
}) as Readonly<LocationQueryRaw>;

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

export const buildDefaultAgentChatRoute = (
  options: { desktop?: boolean } = {}
): RouteLocationRaw => ({
  path: options.desktop === true ? '/desktop/chat' : '/app/chat',
  query: { ...DEFAULT_AGENT_CHAT_QUERY }
});

export const redirectToLoginAfterLogout = (
  replace?: (to: string) => Promise<unknown> | unknown
): void => {
  const target = FORCE_LOGOUT_LOGIN_PATH;
  if (replace) {
    Promise.resolve(replace(target))
      .then((result) => {
        if (typeof window !== 'undefined' && isNavigationFailure(result)) {
          window.location.replace(target);
        }
      })
      .catch(() => {
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
