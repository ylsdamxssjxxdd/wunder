import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listPlazaItems = (params: QueryParams = {}) => api.get('/plaza/items', { params });

export const getPlazaItem = (itemId: ApiId, params: QueryParams = {}) =>
  api.get(`/plaza/items/${encodeURIComponent(String(itemId || '').trim())}`, { params });

export const publishPlazaItem = (payload: ApiPayload) => api.post('/plaza/items', payload);

export const deletePlazaItem = (itemId: ApiId) =>
  api.delete(`/plaza/items/${encodeURIComponent(String(itemId || '').trim())}`);

export const importPlazaItem = (itemId: ApiId, payload: ApiPayload = {}) =>
  api.post(`/plaza/items/${encodeURIComponent(String(itemId || '').trim())}/import`, payload);
