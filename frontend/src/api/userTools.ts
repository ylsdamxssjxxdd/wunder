import api from './http';

import type { AxiosProgressEvent } from 'axios';
import type { ApiPayload } from './types';

type UploadRequestConfig = {
  headers?: Record<string, string>;
  onUploadProgress?: (event: AxiosProgressEvent) => void;
};

export const fetchUserMcpServers = () => api.get('/user_tools/mcp');
export const saveUserMcpServers = (payload: ApiPayload) => api.post('/user_tools/mcp', payload);
export const fetchUserMcpTools = (payload: ApiPayload) => api.post('/user_tools/mcp/tools', payload);

export const fetchUserSkills = () => api.get('/user_tools/skills');
export const saveUserSkills = (payload: ApiPayload) => api.post('/user_tools/skills', payload);
export const fetchUserSkillContent = (name: string) =>
  api.get('/user_tools/skills/content', { params: { name } });
export const exportUserSkillArchive = (name: string) =>
  api.get('/user_tools/skills/export', { params: { name }, responseType: 'blob' });
export const fetchUserSkillFiles = (name: string) =>
  api.get('/user_tools/skills/files', { params: { name } });
export const fetchUserSkillFile = (name: string, path: string) =>
  api.get('/user_tools/skills/file', { params: { name, path } });
export const saveUserSkillFile = (payload: ApiPayload) => api.put('/user_tools/skills/file', payload);
export const fetchUserSkillFsContent = (params: ApiPayload) =>
  api.get('/user_tools/skills/fs', { params });
export const searchUserSkillFs = (params: ApiPayload) =>
  api.get('/user_tools/skills/fs/search', { params });
export const saveUserSkillFsFile = (payload: ApiPayload) =>
  api.put('/user_tools/skills/fs/file', payload);
export const uploadUserSkillFsFiles = (formData: FormData, config: UploadRequestConfig = {}) =>
  api.post('/user_tools/skills/fs/upload', formData, {
    ...config,
    headers: { 'Content-Type': 'multipart/form-data', ...(config.headers || {}) }
  });
export const createUserSkillDir = (payload: ApiPayload) => api.post('/user_tools/skills/dir', payload);
export const moveUserSkillEntry = (payload: ApiPayload) => api.post('/user_tools/skills/move', payload);
export const copyUserSkillEntry = (payload: ApiPayload) => api.post('/user_tools/skills/copy', payload);
export const batchUserSkillAction = (payload: ApiPayload) => api.post('/user_tools/skills/batch', payload);
export const deleteUserSkillEntry = (name: string, path: string) =>
  api.delete('/user_tools/skills/file', { params: { name, path } });
export const downloadUserSkillFile = (name: string, path: string) =>
  api.get('/user_tools/skills/download', { params: { name, path }, responseType: 'blob' });
export const downloadUserSkillArchive = (name: string, path?: string) =>
  api.get('/user_tools/skills/archive', { params: path ? { name, path } : { name }, responseType: 'blob' });
export const deleteUserSkill = (name: string) => api.delete('/user_tools/skills', { params: { name } });
export const uploadUserSkillZip = (file: Blob | File) => {
  const form = new FormData();
  form.append('file', file);
  return api.post('/user_tools/skills/upload', form);
};

export const fetchUserKnowledgeConfig = () => api.get('/user_tools/knowledge');
export const saveUserKnowledgeConfig = (payload: ApiPayload) =>
  api.post('/user_tools/knowledge', payload);
export const fetchUserKnowledgeFiles = (base: string) =>
  api.get('/user_tools/knowledge/files', { params: { base } });
export const fetchUserKnowledgeFile = (base: string, path: string) =>
  api.get('/user_tools/knowledge/file', { params: { base, path } });
export const saveUserKnowledgeFile = (payload: ApiPayload) =>
  api.put('/user_tools/knowledge/file', payload);
export const deleteUserKnowledgeFile = (base: string, path: string) =>
  api.delete('/user_tools/knowledge/file', { params: { base, path } });
export const fetchUserKnowledgeDocs = (base: string) =>
  api.get('/user_tools/knowledge/docs', { params: { base } });
export const fetchUserKnowledgeDoc = (base: string, doc_id: string) =>
  api.get('/user_tools/knowledge/doc', { params: { base, doc_id } });
export const fetchUserKnowledgeChunks = (base: string, doc_id: string) =>
  api.get('/user_tools/knowledge/chunks', { params: { base, doc_id } });
export const embedUserKnowledgeChunk = (payload: ApiPayload) =>
  api.post('/user_tools/knowledge/chunk/embed', payload);
export const deleteUserKnowledgeChunk = (payload: ApiPayload) =>
  api.post('/user_tools/knowledge/chunk/delete', payload);
export const updateUserKnowledgeChunk = (payload: ApiPayload) =>
  api.post('/user_tools/knowledge/chunk/update', payload);
export const testUserKnowledge = (payload: ApiPayload) =>
  api.post('/user_tools/knowledge/test', payload);
export const deleteUserKnowledgeDoc = (base: string, doc_id: string) =>
  api.delete('/user_tools/knowledge/doc', { params: { base, doc_id } });
export const reindexUserKnowledge = (payload: ApiPayload) =>
  api.post('/user_tools/knowledge/reindex', payload);
export const uploadUserKnowledgeFile = (base: string, file: Blob | File) => {
  const form = new FormData();
  form.append('base', base);
  form.append('file', file);
  return api.post('/user_tools/knowledge/upload', form);
};

export const fetchUserToolsSummary = () => api.get('/user_tools/tools');
export const fetchUserToolsCatalog = () => api.get('/user_tools/catalog');
export const saveUserSharedTools = (payload: ApiPayload) =>
  api.post('/user_tools/shared_tools', payload);
