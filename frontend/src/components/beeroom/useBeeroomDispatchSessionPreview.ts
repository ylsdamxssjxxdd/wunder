import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';

import { getSession, getSessionEvents, getSessionHistoryPage, getSessionSubagents } from '@/api/chat';
import type { DispatchRuntimeStatus } from '@/components/beeroom/beeroomCanvasChatModel';
import { resolveBeeroomDispatchPreviewStatus } from '@/components/beeroom/beeroomDispatchPreviewStatus';
import {
  isBeeroomSwarmWorkerShadowItem,
  resolveBeeroomSwarmSubagentProjectionDecision
} from '@/components/beeroom/canvas/beeroomSwarmSubagentProjection';
import {
  resolveBeeroomSwarmWorkerReplyFromHistoryMessages,
  resolveBeeroomSwarmWorkerTerminalState
} from '@/components/beeroom/beeroomSwarmWorkerShadowState';
import { shouldPreserveBeeroomDispatchPreviewOnSyncError } from '@/components/beeroom/beeroomDispatchSessionPolicy';
import {
  ACTIVE_BEEROOM_SUBAGENT_STATUSES,
  collectBeeroomHistoricalSubagentItems,
  flattenBeeroomSessionEventRounds,
  mergeBeeroomMissionSubagentItems,
  resolveBeeroomSessionEventTimestamp,
  type BeeroomMissionSubagentItem,
  normalizeBeeroomMissionSubagentItem
} from '@/components/beeroom/beeroomMissionSubagentState';
import { buildSessionWorkflowItems } from '@/components/beeroom/beeroomTaskWorkflow';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog } from '@/utils/chatDebug';

export type BeeroomDispatchSessionPreview = {
  sessionId: string;
  targetAgentId: string;
  targetName: string;
  status: string;
  summary: string;
  dispatchLabel: string;
  updatedTime: number;
  subagents: BeeroomMissionSubagentItem[];
};

type BeeroomDispatchSessionPreviewCacheRecord = {
  version: number;
  sessionId: string;
  targetAgentId: string;
  targetName: string;
  status: string;
  summary: string;
  dispatchLabel: string;
  updatedTime: number;
  subagents: ReturnType<typeof serializePreviewSubagentItem>[];
};

type SessionEventRecord = import('@/components/beeroom/beeroomMissionSubagentState').BeeroomSessionEventRecord;
type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

const SUBAGENT_LIST_LIMIT = 64;
const PREVIEW_CACHE_VERSION = 2;
const PREVIEW_CACHE_STORAGE_KEY = 'wunder:beeroom-dispatch-preview-cache';
const MAX_PREVIEW_CACHE_ENTRIES = 48;
const ACTIVE_LOCAL_RUNTIME_STATUSES = new Set<DispatchRuntimeStatus>([
  'queued',
  'running',
  'awaiting_approval',
  'resuming'
]);
const ACTIVE_PREVIEW_STATUSES = new Set(['queued', 'running']);
const normalizeText = (value: unknown): string => String(value || '').trim();

const resolvePreviewCacheStorage = (): Storage | null => {
  if (typeof window === 'undefined') return null;
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
};

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const clipText = (value: unknown, limit: number): string => {
  const text = normalizeText(value).replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, Math.max(0, limit - 3))}...`;
};

const summarizeDebugSubagents = (items: BeeroomMissionSubagentItem[]) =>
  items.slice(0, 8).map((item) => ({
    key: item.key,
    runId: item.runId,
    sessionId: item.sessionId,
    runKind: item.runKind,
    requestedBy: item.requestedBy,
    spawnedBy: item.spawnedBy,
    status: item.status,
    terminal: item.terminal,
    failed: item.failed,
    updatedTime: item.updatedTime
  }));

const summarizeCanvasProjectionDecisions = (items: BeeroomMissionSubagentItem[]) =>
  items.slice(0, 8).map((item) => {
    const decision = resolveBeeroomSwarmSubagentProjectionDecision(item);
    return {
      key: item.key,
      sessionId: item.sessionId,
      runId: item.runId,
      runKind: item.runKind,
      requestedBy: item.requestedBy,
      projectable: decision.projectable,
      reason: decision.reason,
      status: item.status
    };
  });

const summarizeDebugError = (error: unknown) => {
  const source = error as { name?: unknown; message?: unknown } | null;
  const name = normalizeText(source?.name);
  const message = normalizeText(source?.message);
  return [name, message].filter(Boolean).join(': ') || normalizeText(error);
};

const serializePreviewSubagentItem = (item: BeeroomMissionSubagentItem) => ({
  key: item.key,
  sessionId: item.sessionId,
  runId: item.runId,
  runKind: item.runKind,
  requestedBy: item.requestedBy,
  spawnedBy: item.spawnedBy,
  agentId: item.agentId,
  title: item.title,
  label: item.label,
  status: item.status,
  summary: item.summary,
  userMessage: item.userMessage,
  assistantMessage: item.assistantMessage,
  errorMessage: item.errorMessage,
  updatedTime: item.updatedTime,
  terminal: item.terminal,
  failed: item.failed,
  depth: item.depth,
  role: item.role,
  controlScope: item.controlScope,
  spawnMode: item.spawnMode,
  strategy: item.strategy,
  dispatchLabel: item.dispatchLabel,
  controllerSessionId: item.controllerSessionId,
  parentSessionId: item.parentSessionId,
  parentTurnRef: item.parentTurnRef,
  parentUserRound: item.parentUserRound,
  parentModelRound: item.parentModelRound,
  workflowItems: Array.isArray(item.workflowItems) ? item.workflowItems : []
});

const cloneDispatchPreview = (
  preview: BeeroomDispatchSessionPreview | null | undefined
): BeeroomDispatchSessionPreview | null => {
  if (!preview) return null;
  return {
    sessionId: normalizeText(preview.sessionId),
    targetAgentId: normalizeText(preview.targetAgentId),
    targetName: normalizeText(preview.targetName),
    status: normalizeText(preview.status),
    summary: normalizeText(preview.summary),
    dispatchLabel: normalizeText(preview.dispatchLabel),
    updatedTime: Number(preview.updatedTime || 0),
    subagents: mergeBeeroomMissionSubagentItems(
      (Array.isArray(preview.subagents) ? preview.subagents : [])
        .map((item) => normalizeBeeroomMissionSubagentItem(item))
        .filter((item: BeeroomMissionSubagentItem | null): item is BeeroomMissionSubagentItem => Boolean(item))
    )
  };
};

const dispatchPreviewCache = new Map<string, BeeroomDispatchSessionPreview>();
let dispatchPreviewCacheHydrated = false;

const persistDispatchPreviewCache = () => {
  const storage = resolvePreviewCacheStorage();
  if (!storage) return;
  try {
    storage.setItem(
      PREVIEW_CACHE_STORAGE_KEY,
      JSON.stringify({
        version: PREVIEW_CACHE_VERSION,
        entries: Array.from(dispatchPreviewCache.entries()).map(([sessionId, preview]) => [
          sessionId,
          {
            version: PREVIEW_CACHE_VERSION,
            sessionId,
            targetAgentId: preview.targetAgentId,
            targetName: preview.targetName,
            status: preview.status,
            summary: preview.summary,
            dispatchLabel: preview.dispatchLabel,
            updatedTime: preview.updatedTime,
            subagents: preview.subagents.map((item) => serializePreviewSubagentItem(item))
          } satisfies BeeroomDispatchSessionPreviewCacheRecord
        ])
      })
    );
  } catch {
    // Ignore privacy-mode and quota failures.
  }
};

const hydrateDispatchPreviewCache = () => {
  if (dispatchPreviewCacheHydrated) return;
  dispatchPreviewCacheHydrated = true;
  const storage = resolvePreviewCacheStorage();
  if (!storage) return;
  try {
    const raw = storage.getItem(PREVIEW_CACHE_STORAGE_KEY);
    if (!raw) return;
    const payload = JSON.parse(raw) as {
      entries?: Array<[unknown, Partial<BeeroomDispatchSessionPreviewCacheRecord>]>;
    } | null;
    const entries = Array.isArray(payload?.entries) ? payload.entries : [];
    entries.forEach((entry) => {
      if (!Array.isArray(entry) || entry.length < 2) return;
      const sessionId = normalizeText(entry[0]);
      if (!sessionId) return;
      const record = entry[1];
      const preview = cloneDispatchPreview({
        sessionId,
        targetAgentId: normalizeText(record?.targetAgentId),
        targetName: normalizeText(record?.targetName),
        status: normalizeText(record?.status),
        summary: normalizeText(record?.summary),
        dispatchLabel: normalizeText(record?.dispatchLabel),
        updatedTime: Number(record?.updatedTime || 0),
        subagents: (Array.isArray(record?.subagents) ? record.subagents : [])
          .map((item) => normalizeBeeroomMissionSubagentItem(item))
          .filter((item: BeeroomMissionSubagentItem | null): item is BeeroomMissionSubagentItem => Boolean(item))
      });
      if (!preview) return;
      dispatchPreviewCache.set(sessionId, preview);
    });
    while (dispatchPreviewCache.size > MAX_PREVIEW_CACHE_ENTRIES) {
      const oldest = dispatchPreviewCache.keys().next();
      if (oldest.done) break;
      dispatchPreviewCache.delete(oldest.value);
    }
  } catch {
    try {
      storage.removeItem(PREVIEW_CACHE_STORAGE_KEY);
    } catch {
      // Ignore cleanup failures.
    }
  }
};

const getCachedDispatchPreview = (sessionId: string): BeeroomDispatchSessionPreview | null => {
  hydrateDispatchPreviewCache();
  const normalizedSessionId = normalizeText(sessionId);
  if (!normalizedSessionId) return null;
  const hit = dispatchPreviewCache.get(normalizedSessionId);
  if (!hit) return null;
  dispatchPreviewCache.delete(normalizedSessionId);
  dispatchPreviewCache.set(normalizedSessionId, hit);
  persistDispatchPreviewCache();
  return cloneDispatchPreview(hit);
};

const setCachedDispatchPreview = (
  sessionId: string,
  preview: BeeroomDispatchSessionPreview | null | undefined
) => {
  hydrateDispatchPreviewCache();
  const normalizedSessionId = normalizeText(sessionId);
  if (!normalizedSessionId) return;
  const next = cloneDispatchPreview(preview || null);
  if (!next) {
    dispatchPreviewCache.delete(normalizedSessionId);
  } else {
    dispatchPreviewCache.set(normalizedSessionId, next);
  }
  while (dispatchPreviewCache.size > MAX_PREVIEW_CACHE_ENTRIES) {
    const oldest = dispatchPreviewCache.keys().next();
    if (oldest.done) break;
    dispatchPreviewCache.delete(oldest.value);
  }
  persistDispatchPreviewCache();
};

const resolveEventName = (event: SessionEventRecord): string =>
  normalizeText(event?.event ?? event?.type).toLowerCase();

const resolveEventPayload = (event: SessionEventRecord): Record<string, unknown> => {
  const source =
    event?.data && typeof event.data === 'object' && !Array.isArray(event.data)
      ? (event.data as Record<string, unknown>)
      : null;
  return source || {};
};

const resolveSummaryFromEvents = (events: SessionEventRecord[]): string => {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    const payload = resolveEventPayload(event);
    const eventName = resolveEventName(event);
    if (eventName === 'final') {
      const answer = clipText(
        payload.answer ?? payload.content ?? payload.reply ?? payload.message ?? event?.title,
        140
      );
      if (answer) return answer;
    }
    if (eventName === 'error') {
      const detail = clipText(payload.detail ?? payload.error ?? payload.message ?? event?.title, 140);
      if (detail) return detail;
    }
    if (eventName === 'progress') {
      const progress = clipText(payload.summary ?? payload.stage ?? event?.title, 120);
      if (progress) return progress;
    }
    if (eventName === 'subagent_status' || eventName === 'subagent_dispatch_item_update') {
      const detail = clipText(
        payload.result ??
          payload.summary ??
          payload.message ??
          asRecord(payload.agent_state)?.message ??
          event?.title,
        140
      );
      if (detail) return detail;
    }
  }
  return '';
};

const resolveDispatchLabel = (events: SessionEventRecord[], summary: string): string => {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    const payload = resolveEventPayload(event);
    const eventName = resolveEventName(event);
    if (eventName === 'progress') {
      const label = clipText(payload.summary ?? event?.title, 42);
      if (label) return label;
    }
    if (eventName === 'tool_call') {
      const toolName = clipText(payload.tool ?? payload.name ?? event?.title, 42);
      if (toolName) return toolName;
    }
  }
  return clipText(summary, 42);
};

const resolvePersistedDispatchLabel = (
  sessionSummary: Record<string, unknown> | null | undefined,
  sessionDetail: Record<string, unknown> | null | undefined
): string =>
  normalizeText(
    sessionSummary?.beeroom_dispatch_label ??
      sessionSummary?.last_user_message_preview ??
      sessionDetail?.beeroom_dispatch_label ??
      sessionDetail?.last_user_message_preview
  );

const buildSubagentIdentity = (item: Pick<BeeroomMissionSubagentItem, 'key' | 'sessionId' | 'runId'>) =>
  normalizeText(item.runId || item.sessionId || item.key);

const extractReplyTextFromEvents = (events: SessionEventRecord[]): string => {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    const payload = resolveEventPayload(event);
    const eventName = resolveEventName(event);
    if (eventName !== 'final' && eventName !== 'error') {
      continue;
    }
    const reply = normalizeText(
      payload.answer ??
        payload.content ??
        payload.reply ??
        payload.message ??
        payload.text ??
        payload.output ??
        event?.title
    );
    if (reply) return reply;
  }
  return '';
};

const extractReplyTextFromWorkflowItems = (
  workflowItems: Array<{
    eventType?: string | null | undefined;
    title?: string | null | undefined;
    detail?: string | null | undefined;
  }>
): string => {
  for (let index = workflowItems.length - 1; index >= 0; index -= 1) {
    const item = workflowItems[index];
    const eventType = normalizeText(item?.eventType).toLowerCase();
    if (eventType && eventType !== 'final' && eventType !== 'llm_output') {
      continue;
    }
    const detailRecord = asRecord(item?.detail);
    const structuredReply = normalizeText(
      detailRecord?.answer ??
        detailRecord?.content ??
        detailRecord?.reply ??
        detailRecord?.message ??
        detailRecord?.text ??
        detailRecord?.output ??
        detailRecord?.final_reply
    );
    if (structuredReply) return structuredReply;
    if (eventType === 'final') {
      const detailText = normalizeText(item?.detail);
      if (detailText) return detailText;
    }
  }
  return '';
};

const mergeStickySwarmWorkerShadowItems = (
  current: BeeroomMissionSubagentItem[],
  sticky: BeeroomMissionSubagentItem[]
) => {
  const stickyOnly = sticky.filter((item) => {
    const identity = buildSubagentIdentity(item);
    if (!identity) return false;
    return !current.some((candidate) => buildSubagentIdentity(candidate) === identity);
  });
  return mergeBeeroomMissionSubagentItems(current, stickyOnly);
};

const hydrateSwarmWorkerShadowItems = async (
  items: BeeroomMissionSubagentItem[],
  signal: AbortSignal,
  t: TranslationFn,
  logDispatchPreview: (event: string, payload?: unknown) => void
) => {
  const hydrated = await Promise.all(
    items.map(async (item) => {
      if (!isBeeroomSwarmWorkerShadowItem(item) || !normalizeText(item.sessionId)) {
        return item;
      }
      try {
        const response = await getSessionEvents(item.sessionId, { signal });
        if (signal.aborted) return item;
        const rounds = Array.isArray(response?.data?.data?.rounds) ? response.data.data.rounds : [];
        const events = flattenBeeroomSessionEventRounds(rounds);
        if (!events.length) {
          return item;
        }
        const workflowItems = buildSessionWorkflowItems(rounds, t);
        let replyText =
          extractReplyTextFromEvents(events) || extractReplyTextFromWorkflowItems(workflowItems);
        const summary = resolveSummaryFromEvents(events);
        const updatedTime = Math.max(
          Number(item.updatedTime || 0),
          ...events.map((event) => resolveBeeroomSessionEventTimestamp(event))
        );
        const running = response?.data?.data?.running === true;
        const terminalState = resolveBeeroomSwarmWorkerTerminalState({
          currentStatus: item.status,
          running,
          events,
          workflowItems
        });
        if (!replyText && terminalState.terminal) {
          try {
            const historyResponse = await getSessionHistoryPage(item.sessionId, { limit: 24 });
            if (!signal.aborted) {
              const historyMessages = Array.isArray(historyResponse?.data?.data?.messages)
                ? historyResponse.data.data.messages
                : [];
              replyText = resolveBeeroomSwarmWorkerReplyFromHistoryMessages(historyMessages);
            }
          } catch (historyError) {
            if ((historyError as { name?: string })?.name !== 'CanceledError' && (historyError as { name?: string })?.name !== 'AbortError') {
              logDispatchPreview('hydrate-swarm-worker-shadow-history-error', {
                sessionId: item.sessionId,
                runId: item.runId,
                error: summarizeDebugError(historyError)
              });
            }
          }
        }
        const next: BeeroomMissionSubagentItem = {
          ...item,
          assistantMessage: replyText || item.assistantMessage,
          summary: summary || item.summary,
          updatedTime,
          status: terminalState.status,
          terminal: terminalState.terminal,
          failed: terminalState.failed,
          workflowItems
        };
        logDispatchPreview('hydrate-swarm-worker-shadow-session', {
          sessionId: item.sessionId,
          runId: item.runId,
          status: next.status,
          workflowItemCount: workflowItems.length,
          replyReady: Boolean(next.assistantMessage),
          updatedTime
        });
        return next;
      } catch (error) {
        if ((error as { name?: string })?.name === 'CanceledError') return item;
        if ((error as { name?: string })?.name === 'AbortError') return item;
        logDispatchPreview('hydrate-swarm-worker-shadow-session-error', {
          sessionId: item.sessionId,
          runId: item.runId,
          error: summarizeDebugError(error)
        });
        return item;
      }
    })
  );
  return mergeBeeroomMissionSubagentItems(hydrated);
};

export const useBeeroomDispatchSessionPreview = (options: {
  sessionId: Ref<string>;
  targetAgentId: Ref<string>;
  targetName: Ref<string>;
  runtimeStatus: Ref<DispatchRuntimeStatus>;
  clearedAfter: Ref<number>;
  t: TranslationFn;
}) => {
  const chatStore = useChatStore();
  const rawPreview = ref<BeeroomDispatchSessionPreview | null>(null);
  const stickySwarmWorkerShadowItems = ref<Record<string, BeeroomMissionSubagentItem>>({});
  const logDispatchPreview = (event: string, payload?: unknown) => {
    chatDebugLog('beeroom.dispatch-preview', event, payload);
  };

  let syncTimer: number | null = null;
  let activeController: AbortController | null = null;

  const clearSyncTimer = () => {
    if (syncTimer !== null && typeof window !== 'undefined') {
      window.clearTimeout(syncTimer);
      syncTimer = null;
    }
  };

  const cancelActiveRequest = () => {
    if (activeController) {
      activeController.abort();
      activeController = null;
    }
  };

  const scheduleSync = (delayMs: number) => {
    clearSyncTimer();
    if (typeof window === 'undefined') return;
    syncTimer = window.setTimeout(() => {
      syncTimer = null;
      void syncDispatchSessionPreview();
    }, Math.max(180, Math.floor(delayMs)));
  };

  const syncDispatchSessionPreview = async () => {
    const sessionId = normalizeText(options.sessionId.value);
    const requestedTargetAgentId = normalizeText(options.targetAgentId.value);
    const requestedTargetName = normalizeText(options.targetName.value);
    if (!sessionId) {
      logDispatchPreview('clear-empty-session', {
        localStatus: options.runtimeStatus.value
      });
      rawPreview.value = null;
      cancelActiveRequest();
      clearSyncTimer();
      return;
    }

    if (normalizeText(rawPreview.value?.sessionId) && normalizeText(rawPreview.value?.sessionId) !== sessionId) {
      stickySwarmWorkerShadowItems.value = {};
    }

    if (!rawPreview.value || normalizeText(rawPreview.value.sessionId) !== sessionId) {
      const cachedPreview = getCachedDispatchPreview(sessionId);
      if (cachedPreview) {
        rawPreview.value = cachedPreview;
        logDispatchPreview('restore-cached-preview', {
          sessionId,
          status: cachedPreview.status,
          subagentCount: cachedPreview.subagents.length,
          updatedTime: cachedPreview.updatedTime
        });
      }
    }

    cancelActiveRequest();
    const controller = new AbortController();
    activeController = controller;
    logDispatchPreview('sync-start', {
      sessionId,
      requestedTargetAgentId,
      requestedTargetName,
      localStatus: options.runtimeStatus.value
    });

    try {
      let eventsError: unknown = null;
      let subagentsError: unknown = null;
      const [sessionResponse, eventsResponse, subagentsResponse] = await Promise.all([
        getSession(sessionId, { signal: controller.signal }).catch(() => null),
        getSessionEvents(sessionId, { signal: controller.signal }).catch((error) => {
          eventsError = error;
          return null;
        }),
        getSessionSubagents(
          sessionId,
          { limit: SUBAGENT_LIST_LIMIT, latest_turn_only: true },
          { signal: controller.signal }
        ).catch((error) => {
          subagentsError = error;
          return null;
        })
      ]);
      if (controller.signal.aborted) return;
      if (!eventsResponse && !subagentsResponse) {
        throw eventsError || subagentsError || new Error('beeroom dispatch preview sync failed');
      }

      const eventsPayload = eventsResponse?.data?.data || {};
      const sessionDetail = sessionResponse?.data?.data || null;
      const storedSessionSummary =
        (Array.isArray(chatStore.sessions)
          ? ((chatStore.sessions.find((item) => normalizeText(item?.id) === sessionId) ||
              null) as Record<string, unknown> | null)
          : null) || null;
      const events = flattenBeeroomSessionEventRounds(eventsPayload.rounds);
      const running = eventsPayload.running === true;
      const liveSubagents = (Array.isArray(subagentsResponse?.data?.data?.items)
        ? subagentsResponse.data.data.items
        : []
      )
        .map((item: unknown) => normalizeBeeroomMissionSubagentItem(item))
        .filter((item: BeeroomMissionSubagentItem | null): item is BeeroomMissionSubagentItem => Boolean(item));
      const historicalSubagents = collectBeeroomHistoricalSubagentItems(events, {
        latestTurnOnly: true
      });
      const mergedSubagents = mergeBeeroomMissionSubagentItems(liveSubagents, historicalSubagents);
      const stickySubagents = Object.values(stickySwarmWorkerShadowItems.value || {}).filter((item) =>
        isBeeroomSwarmWorkerShadowItem(item)
      );
      const subagents = await hydrateSwarmWorkerShadowItems(
        mergeStickySwarmWorkerShadowItems(mergedSubagents, stickySubagents),
        controller.signal,
        options.t,
        logDispatchPreview
      );
      stickySwarmWorkerShadowItems.value = Object.fromEntries(
        subagents
          .filter((item) => isBeeroomSwarmWorkerShadowItem(item))
          .map((item) => [buildSubagentIdentity(item), item])
          .filter(([identity]) => Boolean(identity))
      );
      const canvasProjectableSubagents = subagents.filter((item) =>
        resolveBeeroomSwarmSubagentProjectionDecision(item).projectable
      );
      const resolvedAgentId = normalizeText(sessionDetail?.agent_id ?? '');
      const resolvedAgentName = normalizeText(sessionDetail?.agent_name ?? '');
      const targetAgentId = resolvedAgentId || requestedTargetAgentId;
      const targetName = resolvedAgentName || requestedTargetName || targetAgentId;

      const summary = resolveSummaryFromEvents(events);
      const persistedDispatchLabel = resolvePersistedDispatchLabel(storedSessionSummary, sessionDetail);
      const updatedTime = Math.max(
        ...events.map((event) => {
          const timestampMs = Number(event?.timestamp_ms ?? resolveEventPayload(event).timestamp_ms ?? 0);
          if (Number.isFinite(timestampMs) && timestampMs > 0) return timestampMs / 1000;
          const numeric = Number(event?.timestamp ?? resolveEventPayload(event).timestamp ?? 0);
          if (Number.isFinite(numeric) && numeric > 0) {
            return numeric > 1_000_000_000_000 ? numeric / 1000 : numeric;
          }
          const iso = normalizeText(event?.timestamp);
          if (!iso) return 0;
          const parsed = Date.parse(iso);
          return Number.isFinite(parsed) ? parsed / 1000 : 0;
        }),
        ...subagents.map((item) => Number(item.updatedTime || 0)),
        running || ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.runtimeStatus.value)
          ? Math.floor(Date.now() / 1000)
          : 0
      );
      const previewStatus = resolveBeeroomDispatchPreviewStatus({
        localStatus: options.runtimeStatus.value,
        running,
        events,
        subagents
      });
      const localStatusBeforeOverride = options.runtimeStatus.value;
      rawPreview.value = {
        sessionId,
        targetAgentId,
        targetName,
        status: previewStatus,
        summary,
        dispatchLabel: persistedDispatchLabel || resolveDispatchLabel(events, summary),
        updatedTime,
        subagents
      };
      setCachedDispatchPreview(sessionId, rawPreview.value);
      if (!running && !subagents.some((item) => ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(item.status))) {
        if (previewStatus === 'completed' && ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.runtimeStatus.value)) {
          options.runtimeStatus.value = 'completed';
        } else if (previewStatus === 'failed' && ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.runtimeStatus.value)) {
          options.runtimeStatus.value = 'failed';
        } else if (previewStatus === 'cancelled' && ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.runtimeStatus.value)) {
          options.runtimeStatus.value = 'stopped';
        }
      }
      if (localStatusBeforeOverride !== options.runtimeStatus.value) {
        logDispatchPreview('terminal-override-local-status', {
          sessionId,
          from: localStatusBeforeOverride,
          to: options.runtimeStatus.value,
          previewStatus
        });
      }

      const shouldPoll =
        running ||
        ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.runtimeStatus.value) ||
        subagents.some((item) => ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(item.status));
      logDispatchPreview('sync-result', {
        sessionId,
        targetAgentId,
        targetName,
        running,
        previewStatus,
        localStatus: options.runtimeStatus.value,
        eventCount: events.length,
        subagentCount: subagents.length,
        canvasProjectableSubagentCount: canvasProjectableSubagents.length,
        canvasFilteredSubagentCount: Math.max(0, subagents.length - canvasProjectableSubagents.length),
        liveSubagentCount: liveSubagents.length,
        historicalSubagentCount: historicalSubagents.length,
        updatedTime,
        shouldPoll,
        summary: clipText(summary, 120),
        subagents: summarizeDebugSubagents(subagents),
        canvasProjectionDecisions: summarizeCanvasProjectionDecisions(subagents)
      });
      clearSyncTimer();
      if (shouldPoll) {
        scheduleSync(1250);
      }
    } catch (error) {
      if ((error as { name?: string })?.name === 'CanceledError') return;
      if ((error as { name?: string })?.name === 'AbortError') return;
      const status = Number((error as { response?: { status?: unknown } })?.response?.status || 0);
      const preservePreview = shouldPreserveBeeroomDispatchPreviewOnSyncError({
        status,
        currentPreviewSessionId: String(rawPreview.value?.sessionId || '').trim(),
        requestedSessionId: sessionId
      });
      logDispatchPreview('sync-error', {
        sessionId,
        error: summarizeDebugError(error),
        status,
        preservePreview
      });
      if (!preservePreview) {
        rawPreview.value = null;
        setCachedDispatchPreview(sessionId, null);
      }
      clearSyncTimer();
    } finally {
      if (activeController === controller) {
        activeController = null;
      }
    }
  };

  const dispatchPreview = computed<BeeroomDispatchSessionPreview | null>(() => {
    const preview = rawPreview.value;
    if (!preview) return null;
    const clearedAfter = Number(options.clearedAfter.value || 0);
    if (!clearedAfter) {
      return preview;
    }
    const filteredSubagents = preview.subagents.filter((item) => Number(item.updatedTime || 0) > clearedAfter);
    const previewMoment = Math.max(
      Number(preview.updatedTime || 0),
      ...filteredSubagents.map((item) => Number(item.updatedTime || 0))
    );
    const stillActive =
      ACTIVE_PREVIEW_STATUSES.has(preview.status) ||
      filteredSubagents.some((item) => ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(item.status));
    if (!stillActive && previewMoment > 0 && previewMoment <= clearedAfter && filteredSubagents.length === 0) {
      return null;
    }
    if (filteredSubagents.length === preview.subagents.length) {
      return preview;
    }
    return {
      ...preview,
      subagents: filteredSubagents
    };
  });

  watch(
    () =>
      [
        normalizeText(options.sessionId.value),
        normalizeText(options.targetAgentId.value),
        normalizeText(options.targetName.value),
        options.runtimeStatus.value
      ].join('|'),
    () => {
      clearSyncTimer();
      void syncDispatchSessionPreview();
    },
    { immediate: true }
  );

  onBeforeUnmount(() => {
    const sessionId = normalizeText(options.sessionId.value);
    if (sessionId) {
      setCachedDispatchPreview(sessionId, rawPreview.value);
    }
    clearSyncTimer();
    cancelActiveRequest();
  });

  return {
    dispatchPreview
  };
};
