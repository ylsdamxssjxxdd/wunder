import api from './http';

const uploadWorkspaceFiles = (formData, config = {}) => {
  const headers = { 'Content-Type': 'multipart/form-data', ...(config.headers || {}) };
  return api.post('/workspace/upload', formData, { ...config, headers });
};

export const listWorkspaceEntries = (params) => api.get('/workspace', { params });
export const fetchWorkspaceContent = (params) => api.get('/workspace/content', { params });
export const searchWorkspace = (params) => api.get('/workspace/search', { params });
export const uploadWorkspace = (formData, config = {}) => uploadWorkspaceFiles(formData, config);
export const createWorkspaceDir = (payload) => api.post('/workspace/dir', payload);
export const moveWorkspaceEntry = (payload) => api.post('/workspace/move', payload);
export const copyWorkspaceEntry = (payload) => api.post('/workspace/copy', payload);
export const batchWorkspaceAction = (payload) => api.post('/workspace/batch', payload);
export const saveWorkspaceFile = (payload) => api.post('/workspace/file', payload);
export const deleteWorkspaceEntry = (params) => api.delete('/workspace', { params });
export const downloadWorkspaceFile = (params) =>
  api.get('/workspace/download', { params, responseType: 'blob' });
export const downloadWorkspaceArchive = (params) =>
  api.get('/workspace/archive', { params, responseType: 'blob' });

export const listWunderEntries = (params) => listWorkspaceEntries(params);
export const listWunderWorkspace = (params) => listWorkspaceEntries(params);
export const fetchWunderWorkspaceContent = (params) => fetchWorkspaceContent(params);
export const searchWunderWorkspace = (params) => searchWorkspace(params);
export const uploadWunderWorkspace = (formData, config = {}) => uploadWorkspace(formData, config);
export const createWunderWorkspaceDir = (payload) => createWorkspaceDir(payload);
export const moveWunderWorkspaceEntry = (payload) => moveWorkspaceEntry(payload);
export const copyWunderWorkspaceEntry = (payload) => copyWorkspaceEntry(payload);
export const batchWunderWorkspaceAction = (payload) => batchWorkspaceAction(payload);
export const saveWunderWorkspaceFile = (payload) => saveWorkspaceFile(payload);
export const deleteWunderWorkspaceEntry = (params) => deleteWorkspaceEntry(params);
export const downloadWunderWorkspaceFile = (params) => downloadWorkspaceFile(params);
export const downloadWunderWorkspaceArchive = (params) => downloadWorkspaceArchive(params);
