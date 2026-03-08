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
