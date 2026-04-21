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

const resolveAuthSessionScope = (): 'user_web' | 'admin_web' => {
  if (typeof window === 'undefined') {
    return 'user_web';
  }
  return String(window.location.pathname || '').trim().startsWith('/admin')
    ? 'admin_web'
    : 'user_web';
};

const buildAuthConfig = () => ({
  headers: {
    'x-wunder-session-scope': resolveAuthSessionScope()
  }
});

export const login = (payload: ApiPayload) => api.post('/auth/login', payload, buildAuthConfig());
export const register = (payload: ApiPayload) =>
  api.post('/auth/register', payload, buildAuthConfig());
export const loginDemo = (payload: ApiPayload) =>
  api.post('/auth/demo', payload, buildAuthConfig());
export const resetPassword = (payload: ApiPayload) => api.post('/auth/reset_password', payload);
export const fetchMe = () => api.get('/auth/me');
export const updateProfile = (payload: ApiPayload) => api.patch('/auth/me', payload);
export const resetMyWorkState = () => api.post('/auth/me/reset_work_state');
export const fetchMyPreferences = () => api.get('/auth/me/preferences');
export const updateMyPreferences = (payload: ApiPayload) =>
  api.patch('/auth/me/preferences', payload);
export const fetchOrgUnits = () => api.get('/auth/org_units');
