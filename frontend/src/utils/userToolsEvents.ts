const USER_TOOLS_UPDATED_EVENT = 'wunder:user-tools-updated';

type UserToolsUpdatedDetail = {
  scope?: string;
  action?: string;
};

export const emitUserToolsUpdated = (detail: UserToolsUpdatedDetail = {}) => {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new CustomEvent(USER_TOOLS_UPDATED_EVENT, { detail }));
};

export const onUserToolsUpdated = (
  handler: (event: CustomEvent<UserToolsUpdatedDetail>) => void
) => {
  if (typeof window === 'undefined') return () => {};
  const listener = (event: Event) => {
    handler(event as CustomEvent<UserToolsUpdatedDetail>);
  };
  window.addEventListener(USER_TOOLS_UPDATED_EVENT, listener);
  return () => window.removeEventListener(USER_TOOLS_UPDATED_EVENT, listener);
};

