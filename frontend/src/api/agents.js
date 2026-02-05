import api from './http';

export const listAgents = () => api.get('/agents');
export const listSharedAgents = () => api.get('/agents/shared');
export const listRunningAgents = () => api.get('/agents/running');
export const getAgent = (id) => api.get(`/agents/${id}`);
export const createAgent = (payload) => api.post('/agents', payload);
export const updateAgent = (id, payload) => api.put(`/agents/${id}`, payload);
export const deleteAgent = (id) => api.delete(`/agents/${id}`);
export const getDefaultSession = (id) => api.get(`/agents/${id}/default-session`);
export const setDefaultSession = (id, payload) => api.post(`/agents/${id}/default-session`, payload);
