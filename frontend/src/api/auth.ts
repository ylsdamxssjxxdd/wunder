import api from './http';
import type { AxiosResponse } from 'axios';

import { resolveAccessToken } from '@/api/requestAuth';
import type { ApiPayload } from './types';

let myPreferencesInFlight: { token: string; request: Promise<AxiosResponse> } | null = null;
let myPreferencesCache: { token: string; expiresAt: number; response: AxiosResponse } | null = null;
let myPreferencesVersion = 0;

const MY_PREFERENCES_CACHE_MS = 500;

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
export const fetchAuthSettings = () => api.get('/auth/settings');
export const fetchMe = () => api.get('/auth/me');
export const logout = () => api.post('/auth/logout', {}, buildAuthConfig());
export const updateProfile = (payload: ApiPayload) => api.patch('/auth/me', payload);
export const resetMyWorkState = () => api.post('/auth/me/reset_work_state');
export const fetchMyPreferences = () => {
  const token = resolveAccessToken();
  if (
    myPreferencesCache &&
    myPreferencesCache.token === token &&
    myPreferencesCache.expiresAt > Date.now()
  ) {
    return Promise.resolve(myPreferencesCache.response);
  }
  if (myPreferencesInFlight?.token === token) {
    return myPreferencesInFlight.request;
  }
  const requestVersion = myPreferencesVersion;
  const request = api.get('/auth/me/preferences');
  myPreferencesInFlight = { token, request };
  void request.then(
    (response) => {
      if (requestVersion === myPreferencesVersion) {
        myPreferencesCache = {
          token,
          expiresAt: Date.now() + MY_PREFERENCES_CACHE_MS,
          response
        };
      }
      if (myPreferencesInFlight?.request === request) {
        myPreferencesInFlight = null;
      }
    },
    () => {
      if (myPreferencesInFlight?.request === request) {
        myPreferencesInFlight = null;
      }
    }
  );
  return request;
};
export const updateMyPreferences = (payload: ApiPayload) => {
  myPreferencesVersion += 1;
  myPreferencesCache = null;
  return api.patch('/auth/me/preferences', payload);
};
export const fetchOrgUnits = () => api.get('/auth/org_units');
