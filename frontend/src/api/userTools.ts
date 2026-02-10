import api from './http';

import type { ApiPayload } from './types';

export const fetchUserMcpServers = () => api.get('/user_tools/mcp');
export const saveUserMcpServers = (payload: ApiPayload) => api.post('/user_tools/mcp', payload);
export const fetchUserMcpTools = (payload: ApiPayload) => api.post('/user_tools/mcp/tools', payload);

export const fetchUserSkills = () => api.get('/user_tools/skills');
export const saveUserSkills = (payload: ApiPayload) => api.post('/user_tools/skills', payload);
export const fetchUserSkillContent = (name: string) =>
  api.get('/user_tools/skills/content', { params: { name } });
export const fetchUserSkillFiles = (name: string) =>
  api.get('/user_tools/skills/files', { params: { name } });
export const fetchUserSkillFile = (name: string, path: string) =>
  api.get('/user_tools/skills/file', { params: { name, path } });
export const saveUserSkillFile = (payload: ApiPayload) => api.put('/user_tools/skills/file', payload);
export const deleteUserSkill = (name: string) => api.delete('/user_tools/skills', { params: { name } });
export const uploadUserSkillZip = (file: Blob | File) => {
  const form = new FormData();
  form.append('file', file);
  return api.post('/user_tools/skills/upload', form, {
    headers: { 'Content-Type': 'multipart/form-data' }
  });
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
  return api.post('/user_tools/knowledge/upload', form, {
    headers: { 'Content-Type': 'multipart/form-data' }
  });
};

export const fetchUserToolsSummary = () => api.get('/user_tools/tools');
export const fetchUserToolsCatalog = () => api.get('/user_tools/catalog');
export const saveUserSharedTools = (payload: ApiPayload) =>
  api.post('/user_tools/shared_tools', payload);
