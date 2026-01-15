import api from './http';

export const listAgents = () => api.get('/agents');
export const getAgent = (id) => api.get(`/agents/${id}`);
