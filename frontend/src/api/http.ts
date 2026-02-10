import axios from 'axios';
import type { AxiosError } from 'axios';

import { getDemoToken, isDemoMode } from '@/utils/demo';
import { getCurrentLanguage } from '@/i18n';
import { resolveApiBase } from '@/config/runtime';
import { clearMaintenance, isMaintenanceStatus, markMaintenance } from '@/utils/maintenance';

type HttpError = AxiosError & {
  code?: string;
  response?: {
    status?: number;
  };
};

const asHttpError = (error: unknown): HttpError => (error || {}) as HttpError;

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

const isCanceledRequest = (error: unknown) => asHttpError(error)?.code === 'ERR_CANCELED';

const shouldEnterMaintenance = (error: unknown) => {
  if (!error || isCanceledRequest(error)) return false;
  const source = asHttpError(error);
  const status = source.response?.status;
  if (isMaintenanceStatus(status)) {
    return true;
  }
  if (status) {
    return false;
  }
  if (source.code === 'ECONNABORTED') {
    return false;
  }
  return true;
};

const shouldClearMaintenance = (error: unknown) => {
  const source = asHttpError(error);
  const status = source.response?.status;
  if (!status) return false;
  return !isMaintenanceStatus(status);
};

api.interceptors.response.use(
  (response) => {
    clearMaintenance();
    return response;
  },
  (error: unknown) => {
    const source = asHttpError(error);
    if (shouldEnterMaintenance(source)) {
      markMaintenance({
        status: source.response?.status,
        reason: source.code || 'network'
      });
    } else if (shouldClearMaintenance(source)) {
      clearMaintenance();
    }
    return Promise.reject(source);
  }
);

export default api;
