import { chatQueueState as state, applyItems, rememberQueueIdentities, consumeQueueItem, chatQueueRevision, chatQueueForSession } from './chatQueueLiveState';
export { chatQueueState } from './chatQueueLiveState';
import {
  cancelQueuedTurn,
  getSessionQueue,
  prioritizeQueuedTurn,
  reorderSessionQueue
} from '@/api/chat';
import {
  applyLocalQueueReorder,
  isChatQueueRuntimeEvent,
  projectQueueItems,
  queueItemIdentities,
  resolveEmbeddedChatQueueRuntimeEvent,
  type QueuedTurn
} from './chatQueueProjection';

export type { QueuedTurn };
export { isChatQueueRuntimeEvent };

const QUEUE_REFRESH_DEBOUNCE_MS = 220;
let latestRequestId = 0;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
const cleanText = (value: unknown): string => String(value ?? '').trim();

const readResponseData = (response: unknown): Record<string, unknown> => {
  const body = (response as { data?: unknown } | null)?.data;
  if (!body || typeof body !== 'object') return {};
  const data = (body as Record<string, unknown>).data;
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    return data as Record<string, unknown>;
  }
  return body as Record<string, unknown>;
};

export async function refreshChatQueue(
  sessionId: unknown,
  options: { silent?: boolean } = {}
): Promise<void> {
  const targetId = cleanText(sessionId);
  if (!targetId) {
    resetChatQueue();
    return;
  }
  const requestId = ++latestRequestId;
  const revision = chatQueueRevision(targetId);
  const target = chatQueueForSession(targetId);
  if (state.sessionId !== targetId) {
    if (refreshTimer) clearTimeout(refreshTimer);
    refreshTimer = null;
  }
  state.sessionId = targetId;
  if (!options.silent) target.loading = true;
  try {
    const response = await getSessionQueue(targetId);
    if (requestId !== latestRequestId || state.sessionId !== targetId) return;
    if (revision !== chatQueueRevision(targetId)) {
      scheduleChatQueueRefresh(targetId);
      return;
    }
    applyItems(targetId, projectQueueItems(readResponseData(response).items));
    state.error = '';
  } catch (error) {
    if (requestId !== latestRequestId) return;
    if (!options.silent) {
      state.error = (error as { message?: string })?.message || '';
    }
  } finally {
    target.loading = false;
  }
}

export function scheduleChatQueueRefresh(
  sessionId: unknown,
  delayMs = QUEUE_REFRESH_DEBOUNCE_MS
) {
  const targetId = cleanText(sessionId);
  if (!targetId) return;
  if (refreshTimer) clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => {
    refreshTimer = null;
    if (state.sessionId && state.sessionId !== targetId) return;
    void refreshChatQueue(targetId, { silent: true });
  }, delayMs);
}

export function noteChatQueueRuntimeEvent(sessionId: unknown, eventType: unknown, payload?: unknown): void {
  const targetId = cleanText(sessionId);
  const embedded = resolveEmbeddedChatQueueRuntimeEvent(eventType, payload);
  const direct = isChatQueueRuntimeEvent(eventType);
  if (!direct && !embedded) return;
  // The resolver normalizes both direct lifecycle frames and durable
  // thread_change payloads. Consume a terminal row once at this boundary so
  // the composer cannot retain it while the eventual queue refresh is in
  // flight.
  // Direct queue lifecycle frames are authoritative too. Waiting for the
  // eventual queue endpoint refresh leaves a stale composer row behind and
  // makes its cancel action race a task that has already started (409).
  if (targetId && (embedded || direct)) {
    consumeQueueItem(
      targetId,
      embedded?.payload ?? (payload && typeof payload === 'object' ? payload : {}),
      embedded?.eventType ?? cleanText(eventType).toLowerCase()
    );
  }
  if (state.sessionId === targetId) scheduleChatQueueRefresh(sessionId);
}

export function resetChatQueue(): void {
  latestRequestId += 1;
  if (refreshTimer) {
    clearTimeout(refreshTimer);
    refreshTimer = null;
  }
  state.sessionId = '';
  state.items = [];
  state.loading = false;
  state.error = '';
}

/** Drop the local queue projection when a session-wide stop settles it. */
export function clearChatQueueForSession(sessionId: unknown): void {
  const targetId = cleanText(sessionId);
  if (!targetId) return;
  const target = chatQueueForSession(targetId);
  rememberQueueIdentities(targetId, target.items.flatMap(queueItemIdentities));
  target.revision = chatQueueRevision(targetId) + 1;
  applyItems(targetId, []);
}

export type QueueActionResult = { ok: boolean; message: string; item: QueuedTurn | null };

const resolveActionError = (error: unknown): string => {
  const source = error as {
    message?: string;
    response?: { status?: number; data?: unknown };
  };
  const data = source?.response?.data;
  const body = data && typeof data === 'object' && !Array.isArray(data)
    ? data as Record<string, unknown>
    : {};
  const nested = body.data && typeof body.data === 'object' && !Array.isArray(body.data)
    ? body.data as Record<string, unknown>
    : {};
  const message = body.message ?? body.detail ?? body.error ?? nested.message ?? nested.detail;
  if (typeof message === 'string' && message.trim()) return message.trim();
  if (message && typeof message === 'object') {
    try { return JSON.stringify(message); } catch { /* fall through */ }
  }
  return cleanText(source?.message || (source?.response?.status === 409 ? '队列消息已开始执行或已结束' : ''));
};

const rejected = (message = ''): QueueActionResult => ({ ok: false, message, item: null });

/** 插话：由引擎中断当前执行并优先派发这一条，保留其他排队输入。 */
export async function interjectChatQueueTurn(
  sessionId: unknown,
  queueId: unknown
): Promise<QueueActionResult> {
  const targetId = cleanText(sessionId);
  const cleanedQueueId = cleanText(queueId);
  if (!targetId || !cleanedQueueId) return rejected();
  try {
    await prioritizeQueuedTurn(targetId, cleanedQueueId);
    if (state.sessionId === targetId) await refreshChatQueue(targetId, { silent: true });
    return { ok: true, message: '', item: null };
  } catch (error) {
    scheduleChatQueueRefresh(targetId, 0);
    return rejected(resolveActionError(error));
  }
}

/** 撤下：取消一条还没开跑的排队轮次，返回原文供输入框回填。 */
export async function withdrawChatQueueTurn(
  sessionId: unknown,
  queueId: unknown
): Promise<QueueActionResult> {
  const targetId = cleanText(sessionId);
  const cleanedQueueId = cleanText(queueId);
  if (!targetId || !cleanedQueueId) return rejected();
  const target = chatQueueForSession(targetId);
  const removed = target.items.find((item) => item.queueId === cleanedQueueId) || null;
  try {
    await cancelQueuedTurn(targetId, cleanedQueueId);
    if (removed) rememberQueueIdentities(targetId, queueItemIdentities(removed));
    applyItems(
      targetId,
      projectQueueItems(target.items.filter((item) => item.queueId !== cleanedQueueId))
    );
    scheduleChatQueueRefresh(targetId, 0);
    return { ok: true, message: '', item: removed };
  } catch (error) {
    const status = Number((error as { response?: { status?: unknown } })?.response?.status);
    if (status === 409) {
      // A queue_start/finish may have won the race. Reconcile once before
      // surfacing an error; an absent row means the requested removal already
      // succeeded from the user's point of view.
      try {
        await refreshChatQueue(targetId, { silent: true });
        if (!chatQueueForSession(targetId).items.some((item) => item.queueId === cleanedQueueId)) {
          if (removed) rememberQueueIdentities(targetId, queueItemIdentities(removed));
          // The task has already crossed the queue → execution boundary.  It
          // is no longer a withdrawable composer row, so do not broadcast the
          // stale item to the transcript-pruning path.
          return { ok: true, message: '', item: null };
        }
      } catch { /* preserve the original conflict below */ }
    }
    scheduleChatQueueRefresh(targetId, 0);
    return rejected(resolveActionError(error));
  }
}

/** 拖拽排序：先按新顺序本地生效，再写回派发位次。 */
export async function reorderChatQueueTurns(
  sessionId: unknown,
  orderedQueueIds: unknown[]
): Promise<QueueActionResult> {
  const targetId = cleanText(sessionId);
  const orderedIds = (Array.isArray(orderedQueueIds) ? orderedQueueIds : [])
    .map((value) => cleanText(value))
    .filter(Boolean);
  if (!targetId || orderedIds.length < 2) return rejected();
  const nextItems = applyLocalQueueReorder(chatQueueForSession(targetId).items, orderedIds);
  if (!nextItems) {
    if (state.sessionId === targetId) await refreshChatQueue(targetId, { silent: true });
    return rejected();
  }
  applyItems(targetId, nextItems);
  try {
    await reorderSessionQueue(targetId, { queue_ids: orderedIds });
    scheduleChatQueueRefresh(targetId, 0);
    return { ok: true, message: '', item: null };
  } catch (error) {
    scheduleChatQueueRefresh(targetId, 0);
    return rejected(resolveActionError(error));
  }
}
