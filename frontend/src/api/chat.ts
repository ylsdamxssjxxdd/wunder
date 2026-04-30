import api from './http';

import { resolveAccessToken } from '@/api/requestAuth';
import { resolveApiBase } from '@/config/runtime';

type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;

type SocketProtocolOptions = {
  protocols?: string[] | string;
};

type OpenChatSocketOptions = SocketProtocolOptions & {
  allowQueryToken?: boolean;
  params?: QueryParams;
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
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${protocol}//${window.location.host}${trimmed}`;
  }
  return trimmed;
};

const buildWsUrl = (path: string, params: URLSearchParams): string => {
  const base = resolveWsBase();
  const suffix = params.toString();
  if (!suffix) {
    return `${base}${path}`;
  }
  return `${base}${path}?${suffix}`;
};

const buildWsProtocols = (
  token: string | null,
  options: SocketProtocolOptions = {}
): string[] | null => {
  const protocols: string[] = [];
  const appendProtocol = (value: unknown): void => {
    const cleaned = String(value || '').trim();
    if (!cleaned || /\s/.test(cleaned)) return;
    if (!protocols.includes(cleaned)) {
      protocols.push(cleaned);
    }
  };
  if (Array.isArray(options.protocols)) {
    options.protocols.forEach(appendProtocol);
  } else if (typeof options.protocols === 'string') {
    appendProtocol(options.protocols);
  }
  appendProtocol('wunder');
  if (token) {
    appendProtocol(`wunder-auth.${token}`);
  }
  return protocols.length ? protocols : null;
};

export const createSession = (payload: unknown) => api.post('/chat/sessions', payload);
export const listSessions = (params: QueryParams) => api.get('/chat/sessions', { params });
export const getSession = (id: string, options: { signal?: AbortSignal } = {}) =>
  api.get(`/chat/sessions/${id}`, options);
export const getSessionEvents = (id: string, options: { signal?: AbortSignal } = {}) =>
  api.get(`/chat/sessions/${id}/events`, options);
export const getSessionCommandSessions = (id: string, options: { signal?: AbortSignal } = {}) =>
  api.get(`/chat/sessions/${id}/command-sessions`, options);
export const getSessionCommandSession = (
  sessionId: string,
  commandSessionId: string,
  options: { signal?: AbortSignal } = {}
) => api.get(`/chat/sessions/${sessionId}/command-sessions/${commandSessionId}`, options);
export const getSessionHistoryPage = (id: string, params: QueryParams = {}) =>
  api.get(`/chat/sessions/${id}/history`, { params });
export const getSessionSubagents = (
  id: string,
  params: QueryParams = {},
  options: { signal?: AbortSignal } = {}
) => api.get(`/chat/sessions/${id}/subagents`, { params, ...options });
export const deleteSession = (id: string) => api.delete(`/chat/sessions/${id}`);
export const archiveSession = (id: string) => api.post(`/chat/sessions/${id}/archive`);
export const restoreSession = (id: string) => api.post(`/chat/sessions/${id}/restore`);
export const renameSession = (id: string, payload: unknown) =>
  api.post(`/chat/sessions/${id}/title`, payload);
export const sendMessage = (id: string, payload: unknown) => api.post(`/chat/sessions/${id}/messages`, payload);
export const submitMessageFeedback = (
  sessionId: string,
  historyId: number | string,
  payload: unknown
) => api.post(`/chat/sessions/${sessionId}/messages/${historyId}/feedback`, payload);
export const fetchSessionSystemPrompt = (id: string, payload: unknown) =>
  api.post(`/chat/sessions/${id}/system-prompt`, payload);
export const fetchRealtimeSystemPrompt = (payload: unknown) => api.post('/chat/system-prompt', payload);
export const updateSessionTools = (id: string, payload: unknown) => api.post(`/chat/sessions/${id}/tools`, payload);
export const controlSessionSubagents = (id: string, payload: unknown) =>
  api.post(`/chat/sessions/${id}/subagents/control`, payload);
export const convertChatAttachment = (file: File) => {
  const formData = new FormData();
  formData.append('file', file);
  return api.post('/chat/attachments/convert', formData);
};

export const processChatMediaAttachment = (formData: FormData) =>
  api.post('/chat/attachments/media/process', formData);

export const cancelMessageStream = (id: string) => api.post(`/chat/sessions/${id}/cancel`);
export const compactSession = (
  id: string,
  payload: unknown = {},
  options: { signal?: AbortSignal; timeout?: number } = {}
) =>
  api.post(`/chat/sessions/${id}/compaction`, payload, {
    ...options,
    timeout: options.timeout ?? 0
  });

export const openChatSocket = (options: OpenChatSocketOptions = {}): WebSocket => {
  const token = resolveAccessToken();
  const params = new URLSearchParams();
  const allowQueryToken = options.allowQueryToken === true;
  if (allowQueryToken && token) {
    params.set('access_token', token);
  }
  if (options.params) {
    Object.entries(options.params).forEach(([key, value]) => {
      if (value === undefined || value === null || value === '') return;
      params.set(key, String(value));
    });
  }
  const url = buildWsUrl('/chat/ws', params);
  const protocols = buildWsProtocols(token, options);
  return protocols ? new WebSocket(url, protocols) : new WebSocket(url);
};
