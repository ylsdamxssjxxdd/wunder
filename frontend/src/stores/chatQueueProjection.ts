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

const QUEUE_MARKER_EVENT_TYPES = new Set(['queued', 'queue_enter', 'queue_update']);

const readMessageId = (message: Record<string, any> | null | undefined): string =>
  cleanText(message?.message_id ?? message?.messageId ?? message?.id);

const isQueueMarkerItem = (item: unknown): boolean => {
  const source = (item || {}) as Record<string, unknown>;
  const eventType = cleanText(source.eventType ?? source.event ?? source.event_type).toLowerCase();
  return QUEUE_MARKER_EVENT_TYPES.has(eventType);
};

/** 助手侧除了「排队中」标记什么都没有：这一轮还没真正开跑。 */
export const hasOnlyQueueMarkers = (message: unknown): boolean => {
  const source = message as Record<string, any> | null | undefined;
  if (!source || cleanText(source.role).toLowerCase() !== 'assistant') return false;
  if (cleanText(source.content)) return false;
  if (cleanText(source.reasoning)) return false;
  if (source.workflowStreaming || source.reasoningStreaming) return false;
  const items = Array.isArray(source.workflowItems) ? source.workflowItems : [];
  return items.every(isQueueMarkerItem);
};

/** 历史补水后的排队标记条目 id 形如 `queue:<queue_id>`，用它也能对上排队条。 */
const markerQueueIdsOf = (message: unknown): Set<string> => {
  const source = message as Record<string, any> | null | undefined;
  const items = Array.isArray(source?.workflowItems) ? source.workflowItems : [];
  return new Set(
    items
      .filter(isQueueMarkerItem)
      .map((item) => cleanText((item as Record<string, unknown>)?.id))
      .filter((id) => id.startsWith('queue:'))
      .map((id) => id.slice('queue:'.length))
  );
};

/**
 * 排队条替时间线承载这一轮：条目确实还挂在引擎队列里，且这一轮一行输出都没有。
 * 两个条件都要——只看队列会漏掉「已开跑但条目还没刷掉」，只看状态会在排队接口
 * 查询失败时把消息两头都藏掉。
 */
export const isParkedQueueTurn = (
  items: QueuedTurn[],
  userMessage: unknown,
  assistantMessage: unknown
): boolean => {
  if (!hasOnlyQueueMarkers(assistantMessage)) return false;
  const user = userMessage as Record<string, any> | null | undefined;
  const clientMessageId = cleanText(user?.client_message_id ?? readMessageId(user));
  if (clientMessageId && items.some((item) => item.clientMessageId === clientMessageId)) {
    return true;
  }
  const markerQueueIds = markerQueueIdsOf(assistantMessage);
  return markerQueueIds.size > 0 && items.some((item) => markerQueueIds.has(item.queueId));
};

/**
 * 撤下一条还没开跑的轮次后，要从时间线里剪掉的本地乐观消息 id。
 * 助手侧已经有任何真实输出时返回空：那种情况下这一轮不属于排队条，不该被剪。
 */
export const selectParkedTurnMessageIds = (
  messages: Array<Record<string, any> | null | undefined>,
  clientMessageId: unknown
): string[] => {
  const cleaned = cleanText(clientMessageId);
  const list = Array.isArray(messages) ? messages : [];
  if (!cleaned) return [];
  const user = list.find(
    (message) =>
      message &&
      cleanText(message.role).toLowerCase() === 'user' &&
      cleanText(message.client_message_id ?? readMessageId(message)) === cleaned
  );
  if (!user) return [];
  const userTurnId = cleanText(user.user_turn_id);
  const assistants = list.filter(
    (message) =>
      message &&
      cleanText(message.role).toLowerCase() === 'assistant' &&
      (!userTurnId || cleanText(message.user_turn_id) === userTurnId)
  );
  if (!assistants.length || !assistants.every((assistant) => hasOnlyQueueMarkers(assistant))) {
    return [];
  }
  return [readMessageId(user), ...assistants.map(readMessageId)].filter(Boolean);
};
