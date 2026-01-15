import api from './http';

export const fetchUserMcpServers = () => api.get('/user_tools/mcp');
export const saveUserMcpServers = (payload) => api.post('/user_tools/mcp', payload);
export const fetchUserMcpTools = (payload) => api.post('/user_tools/mcp/tools', payload);

export const fetchUserSkills = () => api.get('/user_tools/skills');
export const saveUserSkills = (payload) => api.post('/user_tools/skills', payload);
export const fetchUserSkillContent = (name) =>
  api.get('/user_tools/skills/content', { params: { name } });
export const uploadUserSkillZip = (file) => {
  const form = new FormData();
  form.append('file', file);
  return api.post('/user_tools/skills/upload', form, {
    headers: { 'Content-Type': 'multipart/form-data' }
  });
};

export const fetchUserKnowledgeConfig = () => api.get('/user_tools/knowledge');
export const saveUserKnowledgeConfig = (payload) => api.post('/user_tools/knowledge', payload);
export const fetchUserKnowledgeFiles = (base) =>
  api.get('/user_tools/knowledge/files', { params: { base } });
export const fetchUserKnowledgeFile = (base, path) =>
  api.get('/user_tools/knowledge/file', { params: { base, path } });
export const saveUserKnowledgeFile = (payload) => api.put('/user_tools/knowledge/file', payload);
export const deleteUserKnowledgeFile = (base, path) =>
  api.delete('/user_tools/knowledge/file', { params: { base, path } });
export const uploadUserKnowledgeFile = (base, file) => {
  const form = new FormData();
  form.append('base', base);
  form.append('file', file);
  return api.post('/user_tools/knowledge/upload', form, {
    headers: { 'Content-Type': 'multipart/form-data' }
  });
};

export const fetchUserToolsSummary = () => api.get('/user_tools/tools');
export const saveUserExtraPrompt = (payload) => api.post('/user_tools/extra_prompt', payload);
