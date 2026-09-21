// Keep structured-clone work and subsequent UI formatting bounded as well as parsing.
export const boundToolDetailPreview = (value: unknown): unknown => {
  let remaining = 32000;
  const visit = (item: unknown, depth: number): unknown => {
    if (remaining <= 0 || depth > 12) return null;
    if (typeof item === 'string') {
      const text = item.slice(0, Math.min(remaining, 24000));
      remaining -= text.length;
      return text;
    }
    if (Array.isArray(item)) return item.slice(0, 120).map(entry => visit(entry, depth + 1));
    if (item && typeof item === 'object') {
      return Object.fromEntries(Object.entries(item).slice(0, 120).map(([key, entry]) => {
        remaining -= key.length + 8;
        return [key, visit(entry, depth + 1)];
      }));
    }
    remaining -= 8;
    return item;
  };
  return visit(value, 0);
};
