import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listAgents = (params: QueryParams = {}) => api.get('/agents', { params });
export const listAgentModels = (params: QueryParams = {}) => api.get('/agents/models', { params });
/**
 * `GET /user/agent` — the user's single agent plus `preset_binding` and the
 * preset-declared `customizable` surface (方案 §12.2.1 3)). Already delivered by
 * the server; the frontend previously only used the legacy `/agents` list.
 */
export const getUserAgent = (params: QueryParams = {}) => api.get('/user/agent', { params });
// 「共享智能体」入口已随 A2/A3 下线（服务端不再提供 /agents/shared），前端不得再调用。
export const listRunningAgents = () => api.get('/agents/running');
export const listAgentUserRounds = () => api.get('/agents/user-rounds');
export const getAgent = (id: ApiId) => api.get(`/agents/${id}`);
export const createAgent = (payload: ApiPayload) => api.post('/agents', payload);
export const updateAgent = (id: ApiId, payload: ApiPayload) => api.put(`/agents/${id}`, payload);
// 删除智能体已随用户侧多智能体下线：服务端 `/wunder/agents/{id}` 只剩 GET/PUT，前端不再暴露 DELETE。
export const getAgentRuntimeRecords = (id: ApiId, params: QueryParams = {}) =>
  api.get(`/agents/${id}/runtime-records`, { params });
