type ChatMessage = Record<string, any>;

// Keep realtime watchers and detail reconcile on the same array reference.
export const replaceMessageArrayKeepingReference = (
  target: ChatMessage[] | null | undefined,
  next: ChatMessage[] | null | undefined
): ChatMessage[] => {
  if (!Array.isArray(next)) {
    if (Array.isArray(target)) {
      target.splice(0, target.length);
      return target;
    }
    return [];
  }
  if (!Array.isArray(target)) {
    return next;
  }
  if (target === next) {
    return target;
  }
  target.splice(0, target.length, ...next);
  return target;
};
