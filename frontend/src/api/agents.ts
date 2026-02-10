import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listAgents = (params: QueryParams = {}) => api.get('/agents', { params });
export const listSharedAgents = () => api.get('/agents/shared');
export const listRunningAgents = () => api.get('/agents/running');
export const getAgent = (id: ApiId) => api.get(`/agents/${id}`);
export const createAgent = (payload: ApiPayload) => api.post('/agents', payload);
export const updateAgent = (id: ApiId, payload: ApiPayload) => api.put(`/agents/${id}`, payload);
export const deleteAgent = (id: ApiId) => api.delete(`/agents/${id}`);
export const getDefaultSession = (id: ApiId) => api.get(`/agents/${id}/default-session`);
export const setDefaultSession = (id: ApiId, payload: ApiPayload) =>
  api.post(`/agents/${id}/default-session`, payload);
