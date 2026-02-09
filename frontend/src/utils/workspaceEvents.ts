const WORKSPACE_REFRESH_EVENT = 'wunder:workspace-refresh';

export const emitWorkspaceRefresh = (detail = {}) => {
  if (typeof window === 'undefined') return;
  const payload = detail && typeof detail === 'object' ? detail : { detail };
  window.dispatchEvent(new CustomEvent(WORKSPACE_REFRESH_EVENT, { detail: payload }));
};

export const onWorkspaceRefresh = (handler) => {
  if (typeof window === 'undefined') return () => {};
  const listener = (event) => {
    if (typeof handler === 'function') {
      handler(event);
    }
  };
  window.addEventListener(WORKSPACE_REFRESH_EVENT, listener);
  return () => window.removeEventListener(WORKSPACE_REFRESH_EVENT, listener);
};
