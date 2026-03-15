import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

const DEFAULT_AGENT_MEMORY_SCOPE = '__default__';

function encodePathSegment(value: ApiId, fallback = ''): string {
  const normalized = String(value ?? '').trim() || fallback;
  return encodeURIComponent(normalized);
}

function encodeAgentScope(agentId: ApiId): string {
  return encodePathSegment(agentId, DEFAULT_AGENT_MEMORY_SCOPE);
}

function encodeMemoryId(memoryId: ApiId): string {
  return encodePathSegment(memoryId);
}

function buildAgentMemoryBasePath(agentId: ApiId): string {
  return `/agents/${encodeAgentScope(agentId)}`;
}

export const listAgentMemories = (agentId: ApiId, params: QueryParams = {}) =>
  api.get(`${buildAgentMemoryBasePath(agentId)}/memories`, { params });
export const getAgentMemory = (agentId: ApiId, memoryId: ApiId) =>
  api.get(`${buildAgentMemoryBasePath(agentId)}/memories/${encodeMemoryId(memoryId)}`);
export const createAgentMemory = (agentId: ApiId, payload: ApiPayload) =>
  api.post(`${buildAgentMemoryBasePath(agentId)}/memories`, payload);
export const updateAgentMemory = (agentId: ApiId, memoryId: ApiId, payload: ApiPayload) =>
  api.patch(`${buildAgentMemoryBasePath(agentId)}/memories/${encodeMemoryId(memoryId)}`, payload);
export const deleteAgentMemory = (agentId: ApiId, memoryId: ApiId) =>
  api.delete(`${buildAgentMemoryBasePath(agentId)}/memories/${encodeMemoryId(memoryId)}`);
export const pinAgentMemory = (agentId: ApiId, memoryId: ApiId, value = true) =>
  api.post(`${buildAgentMemoryBasePath(agentId)}/memories/${encodeMemoryId(memoryId)}/pin`, { value });
export const invalidateAgentMemory = (agentId: ApiId, memoryId: ApiId, value = true) =>
  api.post(`${buildAgentMemoryBasePath(agentId)}/memories/${encodeMemoryId(memoryId)}/invalidate`, { value });
export const listAgentMemoryHits = (agentId: ApiId, params: QueryParams = {}) =>
  api.get(`${buildAgentMemoryBasePath(agentId)}/memory-hits`, { params });
