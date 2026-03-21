type UnknownObject = Record<string, unknown>;

export type WorkflowCompactionSnapshot = {
  eventType: 'compaction_progress' | 'compaction';
  status: 'pending' | 'loading' | 'completed' | 'failed';
  detail: UnknownObject | null;
};

const asObject = (value: unknown): UnknownObject | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as UnknownObject)
    : null;

const parseDetailObject = (value: unknown): UnknownObject | null => {
  if (!value) return null;
  const direct = asObject(value);
  if (direct) return direct;
  if (typeof value !== 'string') return null;
  const text = value.trim();
  if (!text) return null;
  try {
    const parsed = JSON.parse(text);
    return asObject(parsed);
  } catch {
    return null;
  }
};

const normalizeCompactionStatus = (value: unknown): WorkflowCompactionSnapshot['status'] => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'pending') return 'pending';
  if (normalized === 'loading' || normalized === 'running' || normalized === 'in_progress') {
    return 'loading';
  }
  if (normalized === 'failed' || normalized === 'error') return 'failed';
  return 'completed';
};

const isCompactionEventType = (value: unknown): value is WorkflowCompactionSnapshot['eventType'] => {
  const normalized = String(value || '').trim().toLowerCase();
  return normalized === 'compaction_progress' || normalized === 'compaction';
};

const isCompactionToolName = (value: unknown): boolean => {
  const text = String(value || '').trim().toLowerCase();
  if (!text) return false;
  if (text === 'context_compaction' || text === 'context_compact') return true;
  if (text === 'compaction' || text === '上下文压缩') return true;
  if (text.includes('context') && text.includes('compact')) return true;
  return text.includes('上下文') && text.includes('压缩');
};

export const resolveLatestCompactionSnapshot = (items: unknown): WorkflowCompactionSnapshot | null => {
  if (!Array.isArray(items) || items.length === 0) return null;
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = asObject(items[index]);
    if (!item) continue;
    const eventTypeRaw = String(item.eventType || item.event || '').trim().toLowerCase();
    const hasCompactionEvent = isCompactionEventType(eventTypeRaw);
    if (!hasCompactionEvent && !isCompactionToolName(item.toolName || item.tool || item.name)) {
      continue;
    }
    const eventType: WorkflowCompactionSnapshot['eventType'] = hasCompactionEvent
      ? (eventTypeRaw as WorkflowCompactionSnapshot['eventType'])
      : 'compaction';
    const detail =
      parseDetailObject(item.detail)
      || parseDetailObject(item.data)
      || parseDetailObject(item.payload)
      || null;
    return {
      eventType,
      status: normalizeCompactionStatus(item.status ?? detail?.status),
      detail
    };
  }
  return null;
};

export const isCompactionRunningFromWorkflowItems = (items: unknown): boolean => {
  const snapshot = resolveLatestCompactionSnapshot(items);
  if (!snapshot) return false;
  if (snapshot.eventType === 'compaction_progress') return true;
  return snapshot.status === 'loading' || snapshot.status === 'pending';
};

