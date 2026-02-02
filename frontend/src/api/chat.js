import api from './http';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { resolveApiBase } from '@/config/runtime';

const buildUrl = (path) => {
  const base = resolveApiBase() || api.defaults.baseURL || '';
  return `${base.replace(/\/$/, '')}${path}`;
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
