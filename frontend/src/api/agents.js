import api from './http';

export const listAgents = () => api.get('/agents');
export const getAgent = (id) => api.get(`/agents/${id}`);
export const createAgent = (payload) => api.post('/agents', payload);
export const updateAgent = (id, payload) => api.put(`/agents/${id}`, payload);
export const deleteAgent = (id) => api.delete(`/agents/${id}`);
