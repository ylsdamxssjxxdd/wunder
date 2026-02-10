import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const createTeamRun = (payload: ApiPayload) => api.post('/chat/team_runs', payload);
export const listTeamRuns = (params: QueryParams = {}) => api.get('/chat/team_runs', { params });
export const getTeamRun = (teamRunId: ApiId) =>
  api.get(`/chat/team_runs/${encodeURIComponent(teamRunId)}`);
export const cancelTeamRun = (teamRunId: ApiId) =>
  api.post(`/chat/team_runs/${encodeURIComponent(teamRunId)}/cancel`);
export const listSessionTeamRuns = (sessionId: ApiId, params: QueryParams = {}) =>
  api.get(`/chat/sessions/${encodeURIComponent(sessionId)}/team_runs`, { params });

export const listAdminTeamRuns = (params: QueryParams = {}) => api.get('/admin/team_runs', { params });
export const getAdminTeamRun = (teamRunId: ApiId) =>
  api.get(`/admin/team_runs/${encodeURIComponent(teamRunId)}`);
export const listAdminHiveTeamRuns = (hiveId: ApiId, params: QueryParams = {}) =>
  api.get(`/admin/hives/${encodeURIComponent(hiveId)}/team_runs`, { params });
