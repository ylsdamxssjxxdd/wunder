import api from './http';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { resolveApiBase } from '@/config/runtime';

const buildUrl = (path) => {
  const base = resolveApiBase() || api.defaults.baseURL || '';
  return `${base.replace(/\/$/, '')}${path}`;
};

const resolveWsBase = () => {
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

const buildWsUrl = (path, params) => {
  const base = resolveWsBase();
  const suffix = params?.toString();
  if (!suffix) {
    return `${base}${path}`;
  }
  return `${base}${path}?${suffix}`;
};

const buildWsProtocols = (token, options = {}) => {
  const protocols = [];
  const appendProtocol = (value) => {
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

export const createSession = (payload) => api.post('/chat/sessions', payload);
export const listSessions = (params) => api.get('/chat/sessions', { params });
export const getSession = (id) => api.get(`/chat/sessions/${id}`);
export const getSessionEvents = (id) => api.get(`/chat/sessions/${id}/events`);
export const deleteSession = (id) => api.delete(`/chat/sessions/${id}`);
export const sendMessage = (id, payload) => api.post(`/chat/sessions/${id}/messages`, payload);
export const fetchSessionSystemPrompt = (id, payload) =>
  api.post(`/chat/sessions/${id}/system-prompt`, payload);
export const fetchRealtimeSystemPrompt = (payload) => api.post('/chat/system-prompt', payload);
export const updateSessionTools = (id, payload) => api.post(`/chat/sessions/${id}/tools`, payload);
export const convertChatAttachment = (file) => {
  const formData = new FormData();
  formData.append('file', file);
  return api.post('/chat/attachments/convert', formData);
};
export const sendMessageStream = (id, payload, options = {}) => {
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
  });
};

export const resumeMessageStream = (id, options = {}) => {
  const token = isDemoMode() ? getDemoToken() : localStorage.getItem('access_token');
  const params = new URLSearchParams();
  if (Number.isFinite(options.afterEventId) && options.afterEventId > 0) {
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
  });
};

export const cancelMessageStream = (id) => api.post(`/chat/sessions/${id}/cancel`);

export const openChatSocket = (options = {}) => {
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
