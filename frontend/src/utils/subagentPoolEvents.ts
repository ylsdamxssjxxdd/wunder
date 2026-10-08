const eventName = 'wunder:subagent-pool-changed';
export const emitSubagentPoolChanged = (sessionId: string) => {
  if (typeof window !== 'undefined') window.dispatchEvent(new CustomEvent(eventName, { detail: sessionId }));
};
export const onSubagentPoolChanged = (handler: (sessionId: string) => void) => {
  const listener = (event: Event) => handler(String((event as CustomEvent).detail || ''));
  window.addEventListener(eventName, listener);
  return () => window.removeEventListener(eventName, listener);
};
