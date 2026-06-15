import type { QueryParams } from '@/api/types';

type WorkspaceResourceRequestSource = {
  publicPath?: string | null;
  relativePath?: string | null;
  requestUserId?: string | null;
  requestAgentId?: string | null;
  requestContainerId?: number | null;
};

export const resolveWorkspaceResourceRequestPath = (
  resource: WorkspaceResourceRequestSource
): string => {
  const publicPath = String(resource?.publicPath || '').trim();
  if (publicPath) return publicPath;
  return String(resource?.relativePath || '').trim();
};

export const buildWorkspaceResourceRequestParams = (
  resource: WorkspaceResourceRequestSource,
  extra: QueryParams = {}
): QueryParams => {
  const publicPath = String(resource?.publicPath || '').trim();
  const params: QueryParams = {
    path: resolveWorkspaceResourceRequestPath(resource)
  };

  if (!publicPath) {
    const userId = String(resource?.requestUserId || '').trim();
    const agentId = String(resource?.requestAgentId || '').trim();
    const containerId = Number(resource?.requestContainerId);
    if (userId) {
      params.user_id = userId;
    }
    if (agentId) {
      params.agent_id = agentId;
    }
    if (Number.isFinite(containerId)) {
      params.container_id = String(containerId);
    }
  }

  Object.entries(extra).forEach(([key, value]) => {
    if (value !== undefined && value !== null && value !== '') {
      params[key] = value;
    }
  });
  return params;
};
