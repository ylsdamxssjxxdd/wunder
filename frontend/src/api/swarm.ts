import api from './http';

export const createTeamRun = (payload) => api.post('/chat/team_runs', payload);
export const listTeamRuns = (params = {}) => api.get('/chat/team_runs', { params });
export const getTeamRun = (teamRunId) => api.get(`/chat/team_runs/${encodeURIComponent(teamRunId)}`);
export const cancelTeamRun = (teamRunId) =>
  api.post(`/chat/team_runs/${encodeURIComponent(teamRunId)}/cancel`);
export const listSessionTeamRuns = (sessionId, params = {}) =>
  api.get(`/chat/sessions/${encodeURIComponent(sessionId)}/team_runs`, { params });

export const listAdminTeamRuns = (params = {}) => api.get('/admin/team_runs', { params });
export const getAdminTeamRun = (teamRunId) =>
  api.get(`/admin/team_runs/${encodeURIComponent(teamRunId)}`);
export const listAdminHiveTeamRuns = (hiveId, params = {}) =>
  api.get(`/admin/hives/${encodeURIComponent(hiveId)}/team_runs`, { params });
