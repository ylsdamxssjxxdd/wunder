import api from './http';

export const listHives = (params = {}) => api.get('/hives', { params });
export const createHive = (payload) => api.post('/hives', payload);
export const updateHive = (hiveId, payload) => api.patch(`/hives/${encodeURIComponent(hiveId)}`, payload);
export const getHiveSummary = (hiveId, params = {}) =>
  api.get(`/hives/${encodeURIComponent(hiveId)}/summary`, { params });
export const moveHiveAgents = (hiveId, payload) =>
  api.post(`/hives/${encodeURIComponent(hiveId)}/agents`, payload);
