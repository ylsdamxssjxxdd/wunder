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
  rootTurnId?: string;
  userTurnId?: string;
  turnId?: string;
  taskId?: string;
  itemId?: string;
  createdAt: number;
};

export const QUEUE_MAX_ITEMS = 64;

const QUEUE_EVENT_TYPES = new Set([
  'queued',
  'queue_enter',
  'queue_update',
  'queue_start',
  'queue_status',
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

export type EmbeddedChatQueueRuntimeEvent = {
  eventType: 'queue_start' | 'queue_finish' | 'queue_cancel' | 'queue_fail';
  payload: Record<string, unknown>;
};

/** Extract a queue lifecycle event nested in a durable thread_change row. */
export const resolveEmbeddedChatQueueRuntimeEvent = (
  eventType: unknown,
  payload: unknown
): EmbeddedChatQueueRuntimeEvent | null => {
  const normalized = cleanText(eventType).toLowerCase();
  const terminal = new Set(['queue_start', 'queue_finish', 'queue_cancel', 'queue_fail']);
  if (terminal.has(normalized)) {
    return {
      eventType: normalized as EmbeddedChatQueueRuntimeEvent['eventType'],
      payload: payload && typeof payload === 'object' && !Array.isArray(payload)
        ? payload as Record<string, unknown>
        : {}
    };
  }
  if (normalized !== 'thread_change' || !payload || typeof payload !== 'object' || Array.isArray(payload)) return null;
  const source = payload as Record<string, unknown>;
  const nested = source.data && typeof source.data === 'object' && !Array.isArray(source.data)
    ? source.data as Record<string, unknown>
    : {};
  // Durable thread-change frames use either `item` (transition shape) or
  // `payload` (target shape) for the immutable item row.  Both are valid and
  // must feed the same queue tombstone path before the eventual queue refresh.
  const itemValue = source.item ?? nested.item ?? source.payload ?? nested.payload;
  const item = itemValue && typeof itemValue === 'object' && !Array.isArray(itemValue)
    ? itemValue as Record<string, unknown>
    : {};
  const itemPayload = item.payload && typeof item.payload === 'object' && !Array.isArray(item.payload)
    ? item.payload as Record<string, unknown>
    : {};
  const eventData = { ...source, ...nested, ...item, ...itemPayload };
  const kind = cleanText(eventData.kind).toLowerCase();
  const embeddedType = cleanText(eventData.event_type ?? eventData.eventType).toLowerCase();
  if (kind !== 'queue' || !terminal.has(embeddedType)) return null;
  return {
    eventType: embeddedType as EmbeddedChatQueueRuntimeEvent['eventType'],
    payload: eventData
  };
};

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
    rootTurnId: cleanText(source.root_turn_id ?? source.rootTurnId),
    userTurnId: cleanText(source.user_turn_id ?? source.userTurnId),
    turnId: cleanText(source.turn_id ?? source.turnId),
    taskId: cleanText(source.task_id ?? source.taskId),
    itemId: cleanText(source.item_id ?? source.itemId),
    createdAt: readNumber(source.created_at, source.createdAt) ?? 0
  };
};

/** Stable identities shared by live queue events and eventually consistent queue snapshots. */
export const queueItemIdentities = (item: QueuedTurn | Record<string, unknown>): string[] => {
  const source = item as Record<string, unknown>;
  return [
    ['queue', source.queueId ?? source.queue_id],
    ['client', source.clientMessageId ?? source.client_message_id],
    ['root', source.rootTurnId ?? source.root_turn_id],
    ['user', source.userTurnId ?? source.user_turn_id],
    ['task', source.taskId ?? source.task_id],
    ['turn', source.turnId ?? source.turn_id],
    ['item', source.itemId ?? source.item_id]
  ].map(([kind, value]) => {
    const text = cleanText(value);
    return text ? `${kind}:${text}` : '';
  }).filter(Boolean);
};

/** Keep an older queue response from restoring turns already consumed by runtime events. */
export const filterConsumedQueueItems = (
  items: QueuedTurn[],
  consumedIdentities: ReadonlySet<string>
): QueuedTurn[] => consumedIdentities.size
  ? items.filter((item) => !queueItemIdentities(item).some((identity) => consumedIdentities.has(identity)))
  : items;

const queueEventData = (payload: unknown): Record<string, unknown> => {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) return {};
  const source = payload as Record<string, unknown>;
  const nested = source.data && typeof source.data === 'object' && !Array.isArray(source.data)
    ? source.data as Record<string, unknown>
    : {};
  return { ...source, ...nested };
};

/**
 * Queue waiting has two different meanings. `input`/`scheduler` means the
 * current session is still running and this message is only parked behind
 * it; `capacity` means the request is waiting for a global execution slot.
 * Keep this interpretation in one place so transport and durable projections
 * cannot drift apart.
 */
export const resolveQueueWaitReason = (payload: unknown): string => {
  const data = queueEventData(payload);
  const nested = data.queued_input && typeof data.queued_input === 'object' && !Array.isArray(data.queued_input)
    ? data.queued_input as Record<string, unknown>
    : data.queue_wait && typeof data.queue_wait === 'object' && !Array.isArray(data.queue_wait)
      ? data.queue_wait as Record<string, unknown>
      : {};
  return cleanText(
    nested.reason ?? nested.wait_reason ??
    data.reason ?? data.wait_reason ?? data.queue_reason ?? data.queue_wait_reason
  ).toLowerCase();
};

/** Resolve the queue row consumed by a start/terminal event, including legacy identity fallbacks. */
export const resolveConsumedQueueEventIdentities = (
  items: QueuedTurn[],
  eventType: unknown,
  payload: unknown
): string[] => {
  const normalizedType = cleanText(eventType).toLowerCase();
  const data = queueEventData(payload);
  const sourceEventType = cleanText(
    data.source_event_type ?? data.sourceEventType ?? data.event_type ?? data.event
  ).toLowerCase();
  const consumesQueueItem = ['queue_start', 'queue_finish', 'queue_cancel', 'queue_fail'].includes(normalizedType) ||
    sourceEventType === 'queue_start';
  if (!consumesQueueItem) return [];
  const eventIdentities = queueItemIdentities(data);
  let item = eventIdentities.length
    ? items.find((candidate) => queueItemIdentities(candidate).some((identity) => eventIdentities.includes(identity)))
    : null;
  // Legacy queue_start controls occasionally omit all ids. The queue contract
  // defines that transition as the next dispatch head.
  if (!item && !eventIdentities.length && (normalizedType === 'queue_start' || sourceEventType === 'queue_start')) {
    item = items[0] || null;
  }
  if (!item) return eventIdentities;
  return [...new Set([...eventIdentities, ...queueItemIdentities(item)])];
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

const hasExplicitQueuedStatus = (message: unknown): boolean => {
  const source = message as Record<string, any> | null | undefined;
  const statuses = [source?.status, source?.state, source?.runtime_status, source?.runtimeStatus];
  return statuses.some((value) => cleanText(value).toLowerCase() === 'queued');
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
  // The queue strip is the owner of an explicitly queued, still-empty turn.
  // Do not wait for the asynchronous queue list to catch up: during that gap
  // the thread-view row would otherwise flash as a duplicate empty bubble.
  if (hasExplicitQueuedStatus(assistantMessage)) return true;
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
      (userTurnId
        ? cleanText(message.user_turn_id) === userTurnId
        : cleanText(message.client_message_id ?? message.clientMessageId) === cleaned)
  );
  // Empty assistant placeholders occur briefly when a dispatched turn has no
  // output yet.  They are not proof that the turn is still cancellable.  Only
  // a durable queued status or an actual queue lifecycle marker permits local
  // removal.
  if (!assistants.length || !assistants.every((assistant) =>
    hasOnlyQueueMarkers(assistant) &&
    (hasExplicitQueuedStatus(assistant) || markerQueueIdsOf(assistant).size > 0)
  )) {
    return [];
  }
  return [readMessageId(user), ...assistants.map(readMessageId)].filter(Boolean);
};
