import api from './http';

export const fetchCronJobs = (params) => api.get('/cron/list', { params });
export const fetchCronRuns = (job_id, limit) =>
  api.get('/cron/runs', { params: { job_id, limit } });

export const addCronJob = (payload) => api.post('/cron/add', payload);
export const updateCronJob = (payload) => api.post('/cron/update', payload);
export const removeCronJob = (payload) => api.post('/cron/remove', payload);
export const enableCronJob = (payload) => api.post('/cron/enable', payload);
export const disableCronJob = (payload) => api.post('/cron/disable', payload);
export const runCronJob = (payload) => api.post('/cron/run', payload);
export const getCronJob = (payload) => api.post('/cron/get', payload);
export const cronAction = (payload) => api.post('/cron/action', payload);
