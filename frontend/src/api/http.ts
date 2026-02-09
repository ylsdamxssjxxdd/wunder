import axios from 'axios';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { getCurrentLanguage } from '@/i18n';
import { resolveApiBase } from '@/config/runtime';
import { clearMaintenance, isMaintenanceStatus, markMaintenance } from '@/utils/maintenance';

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

const isCanceledRequest = (error) => error?.code === 'ERR_CANCELED';

const shouldEnterMaintenance = (error) => {
  if (!error || isCanceledRequest(error)) return false;
  const status = error?.response?.status;
  if (isMaintenanceStatus(status)) {
    return true;
  }
  if (status) {
    return false;
  }
  if (error?.code === 'ECONNABORTED') {
    return false;
  }
  return true;
};

const shouldClearMaintenance = (error) => {
  const status = error?.response?.status;
  if (!status) return false;
  return !isMaintenanceStatus(status);
};

api.interceptors.response.use(
  (response) => {
    clearMaintenance();
    return response;
  },
  (error) => {
    if (shouldEnterMaintenance(error)) {
      markMaintenance({
        status: error?.response?.status,
        reason: error?.code || 'network'
      });
    } else if (shouldClearMaintenance(error)) {
      clearMaintenance();
    }
    return Promise.reject(error);
  }
);

export default api;
