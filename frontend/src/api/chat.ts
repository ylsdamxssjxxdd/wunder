import api from './http';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { resolveApiBase } from '@/config/runtime';
import { clearMaintenance, isMaintenanceStatus, markMaintenance } from '@/utils/maintenance';

type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;

type SocketProtocolOptions = {
  protocols?: string[] | string;
};

type StreamRequestOptions = {
  signal?: AbortSignal;
};

type ResumeRequestOptions = StreamRequestOptions & {
  afterEventId?: number;
};

type OpenChatSocketOptions = SocketProtocolOptions & {
  allowQueryToken?: boolean;
  params?: QueryParams;
};

const buildUrl = (path: string): string => {
  const base = resolveApiBase() || api.defaults.baseURL || '';
  return `${base.replace(/\/$/, '')}${path}`;
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

const handleStreamResponse = (response: Response): Response => {
  if (response.ok) {
    clearMaintenance();
    return response;
  }
  const status = response.status;
  if (isMaintenanceStatus(status)) {
    markMaintenance({ status, reason: 'http' });
  } else if (status) {
    clearMaintenance();
  }
  return response;
};

const handleStreamError = (error: unknown): never => {
  if (error instanceof DOMException && error.name === 'AbortError') {
    throw error;
  }
  const cause = error as { name?: string; message?: string };
  markMaintenance({ reason: cause?.name || cause?.message || 'network' });
  throw error;
};

export const createSession = (payload: unknown) => api.post('/chat/sessions', payload);
export const fetchChatTransportProfile = () => api.get('/chat/transport');
export const listSessions = (params: QueryParams) => api.get('/chat/sessions', { params });
export const getSession = (id: string) => api.get(`/chat/sessions/${id}`);
export const getSessionEvents = (id: string) => api.get(`/chat/sessions/${id}/events`);
export const deleteSession = (id: string) => api.delete(`/chat/sessions/${id}`);
export const sendMessage = (id: string, payload: unknown) => api.post(`/chat/sessions/${id}/messages`, payload);
export const fetchSessionSystemPrompt = (id: string, payload: unknown) =>
  api.post(`/chat/sessions/${id}/system-prompt`, payload);
export const fetchRealtimeSystemPrompt = (payload: unknown) => api.post('/chat/system-prompt', payload);
export const updateSessionTools = (id: string, payload: unknown) => api.post(`/chat/sessions/${id}/tools`, payload);
export const convertChatAttachment = (file: File) => {
  const formData = new FormData();
  formData.append('file', file);
  return api.post('/chat/attachments/convert', formData);
};

export const sendMessageStream = (
  id: string,
  payload: unknown,
  options: StreamRequestOptions = {}
) => {
  // 浏览器端流式需要使用 fetch 才能读取 SSE 数据
  const token = isDemoMode() ? getDemoToken() : localStorage.getItem('access_token');
  return fetch(buildUrl(`/chat/sessions/${id}/messages`), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { Authorization: `Bearer ${token}` } : {})
    },
    body: JSON.stringify(payload),
    signal: options.signal
  })
    .then(handleStreamResponse)
    .catch(handleStreamError);
};

export const resumeMessageStream = (id: string, options: ResumeRequestOptions = {}) => {
  const token = isDemoMode() ? getDemoToken() : localStorage.getItem('access_token');
  const params = new URLSearchParams();
  if (Number.isFinite(options.afterEventId) && Number(options.afterEventId) >= 0) {
    params.set('after_event_id', String(options.afterEventId));
  }
  const suffix = params.toString();
  const url = suffix
    ? buildUrl(`/chat/sessions/${id}/resume?${suffix}`)
    : buildUrl(`/chat/sessions/${id}/resume`);
  return fetch(url, {
    method: 'GET',
    headers: {
      ...(token ? { Authorization: `Bearer ${token}` } : {})
    },
    signal: options.signal
  })
    .then(handleStreamResponse)
    .catch(handleStreamError);
};

export const cancelMessageStream = (id: string) => api.post(`/chat/sessions/${id}/cancel`);

export const openChatSocket = (options: OpenChatSocketOptions = {}): WebSocket => {
  const token = isDemoMode() ? getDemoToken() : localStorage.getItem('access_token');
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
