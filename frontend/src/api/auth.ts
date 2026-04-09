import api from './http';

import type { ApiPayload } from './types';

export type ResetWorkStateSession = {
  agent_id: string;
  session_id: string;
};

export type ResetWorkStateSummary = {
  cancelled_sessions: number;
  cancelled_tasks: number;
  cancelled_team_runs: number;
  cleared_workspaces: number;
  removed_workspace_entries: number;
  fresh_main_sessions: ResetWorkStateSession[];
};

export const login = (payload: ApiPayload) => api.post('/auth/login', payload);
export const register = (payload: ApiPayload) => api.post('/auth/register', payload);
export const loginDemo = (payload: ApiPayload) => api.post('/auth/demo', payload);
export const fetchMe = () => api.get('/auth/me');
export const updateProfile = (payload: ApiPayload) => api.patch('/auth/me', payload);
export const resetMyWorkState = () => api.post('/auth/me/reset_work_state');
export const fetchMyPreferences = () => api.get('/auth/me/preferences');
export const updateMyPreferences = (payload: ApiPayload) =>
  api.patch('/auth/me/preferences', payload);
export const fetchOrgUnits = () => api.get('/auth/org_units');
