import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listUserPromptTemplates = () => api.get('/prompt_templates');

export const getUserPromptTemplateFile = (params: QueryParams = {}) =>
  api.get('/prompt_templates/file', { params });

export const updateUserPromptTemplateFile = (payload: ApiPayload) =>
  api.put('/prompt_templates/file', payload);

export const setUserPromptTemplateActive = (payload: ApiPayload) =>
  api.post('/prompt_templates/active', payload);

export const createUserPromptTemplatePack = (payload: ApiPayload) =>
  api.post('/prompt_templates/packs', payload);

export const deleteUserPromptTemplatePack = (packId: ApiId) =>
  api.delete(`/prompt_templates/packs/${encodeURIComponent(String(packId || ''))}`);
