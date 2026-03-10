import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listBeeroomGroups = (params: QueryParams = {}) =>
  api.get('/beeroom/groups', { params, timeout: 60000 });

export const createBeeroomGroup = (payload: ApiPayload) =>
  api.post('/beeroom/groups', payload, { timeout: 60000 });

export const getBeeroomGroup = (groupId: ApiId, params: QueryParams = {}) =>
  api.get(`/beeroom/groups/${encodeURIComponent(groupId)}`, { params, timeout: 60000 });

export const moveBeeroomAgents = (groupId: ApiId, payload: ApiPayload) =>
  api.post(`/beeroom/groups/${encodeURIComponent(groupId)}/move_agents`, payload, {
    timeout: 60000
  });

export const listBeeroomMissions = (groupId: ApiId, params: QueryParams = {}) =>
  api.get(`/beeroom/groups/${encodeURIComponent(groupId)}/missions`, { params, timeout: 60000 });

export const getBeeroomMission = (groupId: ApiId, missionId: ApiId) =>
  api.get(`/beeroom/groups/${encodeURIComponent(groupId)}/missions/${encodeURIComponent(missionId)}`, {
    timeout: 60000
  });

export const listBeeroomChatMessages = (groupId: ApiId, params: QueryParams = {}) =>
  api.get(`/beeroom/groups/${encodeURIComponent(groupId)}/chat/messages`, { params, timeout: 60000 });

export const appendBeeroomChatMessage = (groupId: ApiId, payload: ApiPayload) =>
  api.post(`/beeroom/groups/${encodeURIComponent(groupId)}/chat/messages`, payload, { timeout: 60000 });

export const clearBeeroomChatMessages = (groupId: ApiId) =>
  api.delete(`/beeroom/groups/${encodeURIComponent(groupId)}/chat/messages`, { timeout: 60000 });

export type HivePackImportRequest = {
  file: Blob | File;
  options?: ApiPayload;
  groupId?: ApiId;
};

export const importBeeroomHivePack = (payload: HivePackImportRequest) => {
  const form = new FormData();
  form.append('file', payload.file);
  if (payload.options && Object.keys(payload.options).length) {
    form.append('options', JSON.stringify(payload.options));
  }
  const groupId = String(payload.groupId ?? '').trim();
  if (groupId) {
    form.append('group_id', groupId);
  }
  return api.post('/beeroom/packs/import', form, {
    headers: { 'Content-Type': 'multipart/form-data' },
    timeout: 180000
  });
};

export const getBeeroomHivePackImportJob = (jobId: ApiId) =>
  api.get(`/beeroom/packs/import/${encodeURIComponent(String(jobId || ''))}`, {
    timeout: 60000
  });

export const exportBeeroomHivePack = (payload: ApiPayload) =>
  api.post('/beeroom/packs/export', payload, {
    timeout: 180000
  });

export const getBeeroomHivePackExportJob = (jobId: ApiId) =>
  api.get(`/beeroom/packs/export/${encodeURIComponent(String(jobId || ''))}`, {
    timeout: 60000
  });

export const downloadBeeroomHivePack = (jobId: ApiId) =>
  api.get(`/beeroom/packs/export/${encodeURIComponent(String(jobId || ''))}/download`, {
    responseType: 'blob',
    timeout: 180000
  });
