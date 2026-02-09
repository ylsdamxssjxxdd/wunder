import api from './http';

export const listUsers = (params) => api.get('/admin/users', { params });
export const createUser = (payload) => api.post('/admin/users', payload);
export const updateUser = (id, payload) => api.patch(`/admin/users/${id}`, payload);
export const resetUserPassword = (id, payload) => api.post(`/admin/users/${id}/password`, payload);
export const fetchUserToolAccess = (id) => api.get(`/admin/users/${id}/tool-access`);
export const updateUserToolAccess = (id, payload) =>
  api.put(`/admin/users/${id}/tool-access`, payload);

export const listAdminAgents = () => api.get('/admin/agents');
export const createAgent = (payload) => api.post('/admin/agents', payload);
export const updateAgent = (id, payload) => api.put(`/admin/agents/${id}`, payload);

export const fetchSystemStatus = () => api.get('/admin/system/status');

export const fetchWunderSettings = () => api.get('/admin/wunder/settings');
export const updateWunderSettings = (payload) => api.put('/admin/wunder/settings', payload);
export const fetchWunderTools = () => api.get('/admin/wunder/tools');
