import api from './http';

import type { ApiId, ApiPayload, QueryParams } from './types';

export const listChannelAccounts = (params: QueryParams | undefined = undefined) =>
  api.get('/channels/accounts', { params });

export const upsertChannelAccount = (payload: ApiPayload) => api.post('/channels/accounts', payload);

export const deleteChannelAccount = (channel: string, accountId?: ApiId) =>
  accountId
    ? api.delete(`/channels/accounts/${encodeURIComponent(channel)}/${encodeURIComponent(accountId)}`)
    : api.delete(`/channels/accounts/${encodeURIComponent(channel)}`);

export const listChannelBindings = (params: QueryParams | undefined = undefined) =>
  api.get('/channels/bindings', { params });

export const upsertChannelBinding = (payload: ApiPayload) => api.post('/channels/bindings', payload);

export const deleteChannelBinding = (
  channel: string,
  accountId: ApiId,
  peerKind: string,
  peerId: ApiId
) =>
  api.delete(
    `/channels/bindings/${encodeURIComponent(channel)}/${encodeURIComponent(
      accountId
    )}/${encodeURIComponent(peerKind)}/${encodeURIComponent(peerId)}`
  );
