type UnknownObject = Record<string, unknown>;

export type WorkflowCompactionSnapshot = {
  eventType: 'compaction_progress' | 'compaction';
  status: 'pending' | 'loading' | 'completed' | 'failed' | 'cancelled';
  explicitStatus: boolean;
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
  if (normalized === 'cancelled' || normalized === 'canceled' || normalized === 'aborted') {
    return 'cancelled';
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
    const detailStatusRaw = String(detail?.status ?? '').trim();
    const itemStatusRaw = String(item.status ?? '').trim();
    return {
      eventType,
      status: normalizeCompactionStatus(detailStatusRaw || itemStatusRaw),
      explicitStatus: Boolean(detailStatusRaw || itemStatusRaw),
      detail
    };
  }
  return null;
};

export const isCompactionRunningFromWorkflowItems = (items: unknown): boolean => {
  const snapshot = resolveLatestCompactionSnapshot(items);
  if (!snapshot) return false;
  if (snapshot.status === 'loading' || snapshot.status === 'pending') return true;
  if (snapshot.eventType !== 'compaction_progress') return false;
  if (!snapshot.explicitStatus) return true;
  return false;
};

const isCompactionWorkflowItem = (item: unknown): boolean => {
  const record = asObject(item);
  if (!record) return false;
  const eventType = String(record.eventType || record.event || '').trim().toLowerCase();
  if (eventType === 'compaction_notice') return true;
  if (isCompactionEventType(eventType)) return true;
  return isCompactionToolName(record.toolName || record.tool || record.name);
};

export const hasNonCompactionWorkflowItems = (items: unknown): boolean => {
  if (!Array.isArray(items) || items.length === 0) return false;
  for (const item of items) {
    if (!isCompactionWorkflowItem(item)) {
      return true;
    }
  }
  return false;
};

export const isCompactionOnlyWorkflowItems = (items: unknown): boolean => {
  if (!Array.isArray(items) || items.length === 0) return false;
  let hasCompaction = false;
  for (const item of items) {
    if (isCompactionWorkflowItem(item)) {
      hasCompaction = true;
      continue;
    }
    return false;
  }
  return hasCompaction;
};
