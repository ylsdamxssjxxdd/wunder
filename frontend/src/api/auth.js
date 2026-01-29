import api from './http';

export const login = (payload) => api.post('/auth/login', payload);
export const register = (payload) => api.post('/auth/register', payload);
export const loginDemo = (payload) => api.post('/auth/demo', payload);
export const fetchMe = () => api.get('/auth/me');
export const updateProfile = (payload) => api.patch('/auth/me', payload);
