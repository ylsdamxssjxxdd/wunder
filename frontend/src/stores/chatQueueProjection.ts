export type QueuedTurn = {
  queueId: string;
  content: string;
  attachments: unknown[];
  position: number;
  queueAhead: number | null;
  waitAhead: number | null;
  priority: number;
  status: string;
  clientMessageId: string;
  createdAt: number;
};

export const QUEUE_MAX_ITEMS = 64;

const QUEUE_EVENT_TYPES = new Set([
  'queued',
  'queue_enter',
  'queue_update',
  'queue_start',
  'queue_finish',
  'queue_cancel',
  'queue_fail'
]);

const cleanText = (value: unknown): string => String(value ?? '').trim();

const readNumber = (...values: unknown[]): number | null => {
  for (const value of values) {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) return parsed;
  }
  return null;
};

export const isChatQueueRuntimeEvent = (eventType: unknown): boolean =>
  QUEUE_EVENT_TYPES.has(cleanText(eventType).toLowerCase());

export const normalizeQueuedTurn = (value: unknown): QueuedTurn | null => {
  if (!value || typeof value !== 'object') return null;
  const source = value as Record<string, unknown>;
  const queueId = cleanText(source.queue_id ?? source.queueId ?? source.task_id);
  if (!queueId) return null;
  return {
    queueId,
    content: cleanText(source.content),
    attachments: Array.isArray(source.attachments) ? (source.attachments as unknown[]) : [],
    position: readNumber(source.position) ?? 0,
    queueAhead: readNumber(source.queue_ahead, source.queueAhead),
    waitAhead: readNumber(source.wait_ahead, source.waitAhead),
    priority: readNumber(source.priority) ?? 0,
    status: cleanText(source.status) || 'pending',
    clientMessageId: cleanText(source.client_message_id ?? source.clientMessageId),
    createdAt: readNumber(source.created_at, source.createdAt) ?? 0
  };
};

// 引擎列表接口按派发序返回并给出 position；优先级最高的（刚插话的）排最前，
// 其余按 position → created_at → queue_id 稳定排序。
const sortItems = (items: QueuedTurn[]): QueuedTurn[] =>
  [...items].sort((left, right) => {
    if (right.priority !== left.priority) return right.priority - left.priority;
    if (left.position !== right.position) return left.position - right.position;
    if (left.createdAt !== right.createdAt) return left.createdAt - right.createdAt;
    return left.queueId.localeCompare(right.queueId);
  });

const withPositions = (items: QueuedTurn[]): QueuedTurn[] =>
  items.map((item, index) => ({ ...item, position: index }));

/** 引擎排队投影 → 排队条条目：丢弃无效行、按派发序排列并重写位次。 */
export const projectQueueItems = (value: unknown): QueuedTurn[] =>
  withPositions(
    sortItems(
      (Array.isArray(value) ? value : [])
        .map((item) => normalizeQueuedTurn(item))
        .filter((item): item is QueuedTurn => Boolean(item))
    ).slice(0, QUEUE_MAX_ITEMS)
  );

/** 拖拽后的本地序：orderedIds 必须完整覆盖 items，否则返回 null 交给服务端投影。 */
export const applyLocalQueueReorder = (
  items: QueuedTurn[],
  orderedIds: string[]
): QueuedTurn[] | null => {
  const byId = new Map(items.map((item) => [item.queueId, item]));
  const next = orderedIds
    .map((queueId) => {
      const item = byId.get(queueId);
      return item ? { ...item, priority: 0 } : null;
    })
    .filter((item): item is QueuedTurn => Boolean(item));
  if (next.length !== orderedIds.length || next.length !== items.length) return null;
  // 用户给定的顺序就是新的派发顺序，不再按 created_at 重排。
  return withPositions(next);
};
