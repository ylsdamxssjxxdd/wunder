import { reactive } from 'vue';
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
  type QueuedTurn
} from './chatQueueProjection';

export type { QueuedTurn };
export { isChatQueueRuntimeEvent };

type QueueState = {
  sessionId: string;
  items: QueuedTurn[];
  loading: boolean;
  error: string;
};

const QUEUE_REFRESH_DEBOUNCE_MS = 220;

const state = reactive<QueueState>({
  sessionId: '',
  items: [],
  loading: false,
  error: ''
});

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

const applyItems = (sessionId: string, items: QueuedTurn[]) => {
  if (state.sessionId !== sessionId) return;
  state.items = items;
};

export const chatQueueState = state;

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
  state.sessionId = targetId;
  if (!options.silent) state.loading = true;
  try {
    const response = await getSessionQueue(targetId);
    if (requestId !== latestRequestId || state.sessionId !== targetId) return;
    applyItems(targetId, projectQueueItems(readResponseData(response).items));
    state.error = '';
  } catch (error) {
    if (requestId !== latestRequestId) return;
    if (!options.silent) {
      state.error = (error as { message?: string })?.message || '';
    }
  } finally {
    if (requestId === latestRequestId) state.loading = false;
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
    void refreshChatQueue(targetId, { silent: true });
  }, delayMs);
}

export function noteChatQueueRuntimeEvent(sessionId: unknown, eventType: unknown): void {
  if (!isChatQueueRuntimeEvent(eventType)) return;
  scheduleChatQueueRefresh(sessionId);
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

export type QueueActionResult = { ok: boolean; message: string; item: QueuedTurn | null };

const resolveActionError = (error: unknown): string => {
  const source = error as {
    message?: string;
    response?: { data?: { message?: string; detail?: string } };
  };
  return cleanText(
    source?.response?.data?.message || source?.response?.data?.detail || source?.message
  );
};

const rejected = (message = ''): QueueActionResult => ({ ok: false, message, item: null });

/** 插话：把这一条提到队首，在当前动作边界优先执行。 */
export async function interjectChatQueueTurn(
  sessionId: unknown,
  queueId: unknown
): Promise<QueueActionResult> {
  const targetId = cleanText(sessionId);
  const cleanedQueueId = cleanText(queueId);
  if (!targetId || !cleanedQueueId) return rejected();
  try {
    await prioritizeQueuedTurn(targetId, cleanedQueueId);
    await refreshChatQueue(targetId, { silent: true });
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
  const removed = state.items.find((item) => item.queueId === cleanedQueueId) || null;
  try {
    await cancelQueuedTurn(targetId, cleanedQueueId);
    applyItems(
      targetId,
      projectQueueItems(state.items.filter((item) => item.queueId !== cleanedQueueId))
    );
    scheduleChatQueueRefresh(targetId, 0);
    return { ok: true, message: '', item: removed };
  } catch (error) {
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
  const nextItems = applyLocalQueueReorder(state.items, orderedIds);
  if (!nextItems) {
    await refreshChatQueue(targetId, { silent: true });
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
