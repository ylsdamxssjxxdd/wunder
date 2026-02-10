import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listUsers = (params: QueryParams = {}) => api.get('/admin/users', { params });
export const createUser = (payload: ApiPayload) => api.post('/admin/users', payload);
export const updateUser = (id: ApiId, payload: ApiPayload) => api.patch(`/admin/users/${id}`, payload);
export const resetUserPassword = (id: ApiId, payload: ApiPayload) =>
  api.post(`/admin/users/${id}/password`, payload);
export const fetchUserToolAccess = (id: ApiId) => api.get(`/admin/users/${id}/tool-access`);
export const updateUserToolAccess = (id: ApiId, payload: ApiPayload) =>
  api.put(`/admin/users/${id}/tool-access`, payload);

export const listAdminAgents = () => api.get('/admin/agents');
export const createAgent = (payload: ApiPayload) => api.post('/admin/agents', payload);
export const updateAgent = (id: ApiId, payload: ApiPayload) => api.put(`/admin/agents/${id}`, payload);

export const fetchSystemStatus = () => api.get('/admin/system/status');

export const fetchWunderSettings = () => api.get('/admin/wunder/settings');
export const updateWunderSettings = (payload: ApiPayload) => api.put('/admin/wunder/settings', payload);
export const fetchWunderTools = () => api.get('/admin/wunder/tools');
