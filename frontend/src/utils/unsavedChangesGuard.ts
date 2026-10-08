type UnsavedChangesGuard = () => boolean | Promise<boolean>;

const guardRegistry = new Map<string, UnsavedChangesGuard>();

export const registerUnsavedChangesGuard = (key: string, guard: UnsavedChangesGuard): (() => void) => {
  const normalizedKey = String(key || '').trim();
  if (!normalizedKey) {
    return () => undefined;
  }
  guardRegistry.set(normalizedKey, guard);
  return () => {
    const current = guardRegistry.get(normalizedKey);
    if (current === guard) {
      guardRegistry.delete(normalizedKey);
    }
  };
};

export const runUnsavedChangesGuards = async (excludeKey = ''): Promise<boolean> => {
  const normalizedExclude = String(excludeKey || '').trim();
  for (const [key, guard] of guardRegistry.entries()) {
    if (normalizedExclude && key === normalizedExclude) continue;
    const allowed = await Promise.resolve(guard());
    if (!allowed) {
      return false;
    }
  }
  return true;
};
