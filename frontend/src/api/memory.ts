import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listAgentMemories = (agentId: ApiId, params: QueryParams = {}) =>
  api.get(`/agents/${agentId}/memories`, { params });
export const getAgentMemory = (agentId: ApiId, memoryId: ApiId) =>
  api.get(`/agents/${agentId}/memories/${memoryId}`);
export const createAgentMemory = (agentId: ApiId, payload: ApiPayload) =>
  api.post(`/agents/${agentId}/memories`, payload);
export const updateAgentMemory = (agentId: ApiId, memoryId: ApiId, payload: ApiPayload) =>
  api.patch(`/agents/${agentId}/memories/${memoryId}`, payload);
export const deleteAgentMemory = (agentId: ApiId, memoryId: ApiId) =>
  api.delete(`/agents/${agentId}/memories/${memoryId}`);
export const confirmAgentMemory = (agentId: ApiId, memoryId: ApiId, value = true) =>
  api.post(`/agents/${agentId}/memories/${memoryId}/confirm`, { value });
export const pinAgentMemory = (agentId: ApiId, memoryId: ApiId, value = true) =>
  api.post(`/agents/${agentId}/memories/${memoryId}/pin`, { value });
export const invalidateAgentMemory = (agentId: ApiId, memoryId: ApiId, value = true) =>
  api.post(`/agents/${agentId}/memories/${memoryId}/invalidate`, { value });
export const listAgentMemoryHits = (agentId: ApiId, params: QueryParams = {}) =>
  api.get(`/agents/${agentId}/memory-hits`, { params });
