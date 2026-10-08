import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const fetchCronJobs = (params: QueryParams | undefined = undefined) =>
  api.get('/cron/list', { params });

export const fetchCronRuns = (jobId: ApiId, params: QueryParams = {}) =>
  api.get('/cron/runs', { params: { job_id: jobId, ...(params || {}) } });

export const addCronJob = (payload: ApiPayload) => api.post('/cron/add', payload);
export const updateCronJob = (payload: ApiPayload) => api.post('/cron/update', payload);
export const removeCronJob = (payload: ApiPayload) => api.post('/cron/remove', payload);
export const enableCronJob = (payload: ApiPayload) => api.post('/cron/enable', payload);
export const disableCronJob = (payload: ApiPayload) => api.post('/cron/disable', payload);
export const runCronJob = (payload: ApiPayload) => api.post('/cron/run', payload);
export const getCronJob = (payload: ApiPayload) => api.post('/cron/get', payload);
export const cronAction = (payload: ApiPayload) => api.post('/cron/action', payload);
