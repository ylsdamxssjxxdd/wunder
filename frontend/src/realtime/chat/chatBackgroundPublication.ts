type Pending = { timer: ReturnType<typeof setTimeout>; flush: () => void };
const pending = new Map<object, Pending>();

// Coalesce notifications, never events. The canonical reducer always runs immediately.
export const scheduleBackgroundPublication = (owner: object, flush: () => void) => {
  if (pending.has(owner)) return;
  pending.set(owner, { flush, timer: setTimeout(() => {
    pending.delete(owner);
    flush();
  }, 250) });
};

export const flushBackgroundPublication = (owner: object) => {
  const entry = pending.get(owner);
  if (!entry) return;
  clearTimeout(entry.timer);
  pending.delete(owner);
  entry.flush();
};

export const clearBackgroundPublications = () => {
  pending.forEach(entry => clearTimeout(entry.timer));
  pending.clear();
};
