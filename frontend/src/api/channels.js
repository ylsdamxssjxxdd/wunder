import api from './http';

export const listChannelAccounts = (params) => api.get('/channels/accounts', { params });

export const listChannelBindings = (params) => api.get('/channels/bindings', { params });

export const upsertChannelBinding = (payload) => api.post('/channels/bindings', payload);

export const deleteChannelBinding = (channel, accountId, peerKind, peerId) =>
  api.delete(
    `/channels/bindings/${encodeURIComponent(channel)}/${encodeURIComponent(
      accountId
    )}/${encodeURIComponent(peerKind)}/${encodeURIComponent(peerId)}`
  );
