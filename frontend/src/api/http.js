import axios from 'axios';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { getCurrentLanguage } from '@/i18n';
import { resolveApiBase } from '@/config/runtime';

const api = axios.create({
  timeout: 30000
});

api.interceptors.request.use((config) => {
  const apiBase = resolveApiBase();
  if (apiBase) {
    config.baseURL = apiBase;
  }
  const token = isDemoMode() ? getDemoToken() : localStorage.getItem('access_token');
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  const language = getCurrentLanguage();
  if (language) {
    config.headers['x-wunder-language'] = language;
    config.headers['accept-language'] = language;
  }
  return config;
});

export default api;
