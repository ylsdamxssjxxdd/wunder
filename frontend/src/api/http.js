import axios from 'axios';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { getCurrentLanguage } from '@/i18n';

const api = axios.create({
  baseURL:
    import.meta.env.VITE_API_BASE_URL ||
    import.meta.env.VITE_API_BASE ||
    'http://localhost:18000/wunder',
  timeout: 30000
});

api.interceptors.request.use((config) => {
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
