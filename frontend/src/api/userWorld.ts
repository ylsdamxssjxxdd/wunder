import api from './http';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { resolveApiBase } from '@/config/runtime';

type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;

type OpenSocketOptions = {
  protocols?: string[] | string;
  allowQueryToken?: boolean;
  params?: QueryParams;
};

type StreamOptions = {
  signal?: AbortSignal;
  afterEventId?: number;
  limit?: number;
};

const buildUrl = (path: string): string => {
  const base = resolveApiBase() || api.defaults.baseURL || '';
  return `${base.replace(/\/$/, '')}${path}`;
};

const resolveDevProxyWsBase = (apiPathPrefix: string): string => {
  const fallbackTarget = 'http://127.0.0.1:18000';
  const raw = String((import.meta as { env?: Record<string, unknown> }).env?.VITE_DEV_PROXY_TARGET || fallbackTarget)
    .trim();
  try {
    const url = new URL(raw, window.location.origin);
    const protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    const targetPath = url.pathname.replace(/\/$/, '');
    if (!targetPath || apiPathPrefix.startsWith(targetPath)) {
      return `${protocol}//${url.host}${apiPathPrefix}`;
    }
    return `${protocol}//${url.host}${targetPath}${apiPathPrefix}`;
  } catch {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${protocol}//127.0.0.1:18000${apiPathPrefix}`;
  }
};

const resolveWsBase = (): string => {
  const base = resolveApiBase() || api.defaults.baseURL || '';
  const trimmed = base.replace(/\/$/, '');
  if (!trimmed) {
    return '';
  }
  if (/^https?:\/\//i.test(trimmed)) {
    return trimmed.replace(/^http/i, 'ws');
  }
  if (trimmed.startsWith('/')) {
    if (import.meta.env.DEV && window.location.port === '18001') {
      return resolveDevProxyWsBase(trimmed);
    }
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${protocol}//${window.location.host}${trimmed}`;
  }
  return trimmed;
};

const buildWsProtocols = (token: string | null, options: OpenSocketOptions = {}): string[] => {
  const protocols: string[] = [];
  const append = (value: unknown) => {
    const cleaned = String(value || '').trim();
    if (!cleaned || /\s/.test(cleaned)) return;
    if (!protocols.includes(cleaned)) {
      protocols.push(cleaned);
    }
  };
  if (Array.isArray(options.protocols)) {
    options.protocols.forEach(append);
  } else if (typeof options.protocols === 'string') {
    append(options.protocols);
  }
  append('wunder');
  if (token && !options.allowQueryToken) {
    append(`wunder-auth.${token}`);
  }
  return protocols;
};

const buildWsUrl = (path: string, params: URLSearchParams): string => {
  const base = resolveWsBase();
  const suffix = params.toString();
  return suffix ? `${base}${path}?${suffix}` : `${base}${path}`;
};

export const listUserWorldContacts = (params: QueryParams = {}) =>
  api.get('/user_world/contacts', { params, timeout: 60000 });

export const listUserWorldGroups = (params: QueryParams = {}) =>
  api.get('/user_world/groups', { params, timeout: 60000 });

export const createUserWorldGroup = (payload: {
  group_name: string;
  member_user_ids: string[];
}) => api.post('/user_world/groups', payload, { timeout: 60000 });

export const createOrGetUserWorldConversation = (payload: { peer_user_id: string }) =>
  api.post('/user_world/conversations', payload, { timeout: 60000 });

export const listUserWorldConversations = (params: QueryParams = {}) =>
  api.get('/user_world/conversations', { params, timeout: 60000 });

export const getUserWorldConversation = (conversationId: string) =>
  api.get(`/user_world/conversations/${conversationId}`, { timeout: 60000 });

export const listUserWorldMessages = (conversationId: string, params: QueryParams = {}) =>
  api.get(`/user_world/conversations/${conversationId}/messages`, { params, timeout: 60000 });

export const sendUserWorldMessage = (
  conversationId: string,
  payload: {
    content: string;
    content_type?: string;
    client_msg_id?: string;
  }
) => api.post(`/user_world/conversations/${conversationId}/messages`, payload, { timeout: 90000 });

export const markUserWorldRead = (
  conversationId: string,
  payload: {
    last_read_message_id?: number | null;
  } = {}
) => api.post(`/user_world/conversations/${conversationId}/read`, payload, { timeout: 60000 });

export const streamUserWorldEvents = (
  conversationId: string,
  options: StreamOptions = {}
) => {
  const token = isDemoMode() ? getDemoToken() : localStorage.getItem('access_token');
  const params = new URLSearchParams();
  if (Number.isFinite(options.afterEventId) && Number(options.afterEventId) >= 0) {
    params.set('after_event_id', String(options.afterEventId));
  }
  if (Number.isFinite(options.limit) && Number(options.limit) > 0) {
    params.set('limit', String(options.limit));
  }
  const suffix = params.toString();
  const url = suffix
    ? buildUrl(`/user_world/conversations/${conversationId}/events?${suffix}`)
    : buildUrl(`/user_world/conversations/${conversationId}/events`);
  return fetch(url, {
    method: 'GET',
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {})
    },
    signal: options.signal
  });
};

export const downloadUserWorldFile = (params: QueryParams = {}) =>
  api.get('/user_world/files/download', { params, responseType: 'blob' });

export const openUserWorldSocket = (options: OpenSocketOptions = {}): WebSocket => {
  const token = isDemoMode() ? getDemoToken() : localStorage.getItem('access_token');
  const params = new URLSearchParams();
  if (options.allowQueryToken && token) {
    params.set('access_token', token);
  }
  if (options.params) {
    Object.entries(options.params).forEach(([key, value]) => {
      if (value === undefined || value === null || value === '') return;
      params.set(key, String(value));
    });
  }
  const protocols = buildWsProtocols(token, options);
  const url = buildWsUrl('/user_world/ws', params);
  return protocols.length ? new WebSocket(url, protocols) : new WebSocket(url);
};
