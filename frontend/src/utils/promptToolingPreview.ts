type PromptToolingPreview = {
  mode: string;
  text: string;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

const normalizeMode = (value: unknown): string => {
  const mode = String(value || '').trim().toLowerCase();
  if (mode === 'function_call' || mode === 'tool_call' || mode === 'freeform_call') {
    return mode;
  }
  return '';
};

const safeStringify = (value: unknown): string => {
  const seen = new WeakSet<object>();
  try {
    const text = JSON.stringify(
      value,
      (_key, current: unknown) => {
        if (typeof current === 'bigint') {
          return current.toString();
        }
        if (current && typeof current === 'object') {
          const objectValue = current as object;
          if (seen.has(objectValue)) {
            return '[Circular]';
          }
          seen.add(objectValue);
          if (current instanceof Map) {
            return Object.fromEntries(current.entries());
          }
          if (current instanceof Set) {
            return Array.from(current.values());
          }
        }
        return current;
      },
      2
    );
    return typeof text === 'string' ? text : '';
  } catch {
    return '';
  }
};

export const extractPromptToolingPreview = (payload: unknown): PromptToolingPreview => {
  const source = asRecord(payload);
  const tooling = asRecord(source.tooling_preview);
  if (!Object.keys(tooling).length) {
    return { mode: '', text: '' };
  }
  return {
    mode: normalizeMode(tooling.tool_call_mode),
    text: safeStringify(tooling)
  };
};
