import api from './http';

import type { ApiPayload } from './types';

export const login = (payload: ApiPayload) => api.post('/auth/login', payload);
export const register = (payload: ApiPayload) => api.post('/auth/register', payload);
export const loginDemo = (payload: ApiPayload) => api.post('/auth/demo', payload);
export const fetchMe = () => api.get('/auth/me');
export const updateProfile = (payload: ApiPayload) => api.patch('/auth/me', payload);
export const fetchOrgUnits = () => api.get('/auth/org_units');
