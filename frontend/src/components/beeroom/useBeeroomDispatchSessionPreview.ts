import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';

import { getSession, getSessionEvents, getSessionSubagents } from '@/api/chat';
import type { DispatchRuntimeStatus } from '@/components/beeroom/beeroomCanvasChatModel';
import {
  type BeeroomMissionSubagentItem,
  normalizeBeeroomMissionSubagentItem
} from '@/components/beeroom/useBeeroomMissionSubagentPreview';

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

type SessionEventRecord = {
  event?: unknown;
  type?: unknown;
  data?: unknown;
  title?: unknown;
  timestamp?: unknown;
  timestamp_ms?: unknown;
};

type SessionEventRound = {
  events?: SessionEventRecord[];
};

const SUBAGENT_LIST_LIMIT = 64;
const ACTIVE_LOCAL_RUNTIME_STATUSES = new Set<DispatchRuntimeStatus>([
  'queued',
  'running',
  'awaiting_approval',
  'resuming'
]);
const ACTIVE_PREVIEW_STATUSES = new Set(['queued', 'running']);
const ACTIVE_SUBAGENT_STATUSES = new Set(['running', 'waiting', 'queued', 'accepted', 'cancelling']);

const normalizeText = (value: unknown): string => String(value || '').trim();

const clipText = (value: unknown, limit: number): string => {
  const text = normalizeText(value).replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, Math.max(0, limit - 3))}...`;
};

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const resolveEventName = (event: SessionEventRecord): string =>
  normalizeText(event?.event ?? event?.type).toLowerCase();

const resolveEventPayload = (event: SessionEventRecord): Record<string, unknown> => {
  const source = asRecord(event?.data);
  return source || {};
};

const resolveEventTimestamp = (event: SessionEventRecord): number => {
  const timestampMs = Number(event?.timestamp_ms ?? resolveEventPayload(event).timestamp_ms ?? 0);
  if (Number.isFinite(timestampMs) && timestampMs > 0) {
    return timestampMs / 1000;
  }
  const numeric = Number(event?.timestamp ?? resolveEventPayload(event).timestamp ?? 0);
  if (Number.isFinite(numeric) && numeric > 0) {
    return numeric > 1_000_000_000_000 ? numeric / 1000 : numeric;
  }
  const iso = normalizeText(event?.timestamp);
  if (!iso) return 0;
  const parsed = Date.parse(iso);
  return Number.isFinite(parsed) ? parsed / 1000 : 0;
};

const flattenRounds = (rounds: unknown): SessionEventRecord[] => {
  const items: SessionEventRecord[] = [];
  (Array.isArray(rounds) ? rounds : []).forEach((round) => {
    const source = round as SessionEventRound;
    if (!Array.isArray(source?.events)) return;
    source.events.forEach((event) => {
      if (!event || typeof event !== 'object') return;
      items.push(event);
    });
  });
  return items;
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

const resolvePreviewStatus = (
  localStatus: DispatchRuntimeStatus,
  running: boolean,
  events: SessionEventRecord[],
  subagents: BeeroomMissionSubagentItem[]
): string => {
  if (localStatus === 'queued') return 'queued';
  if (ACTIVE_LOCAL_RUNTIME_STATUSES.has(localStatus)) return 'running';
  if (localStatus === 'completed') return 'completed';
  if (localStatus === 'failed') return 'failed';
  if (localStatus === 'stopped') return 'cancelled';
  if (running) return 'running';
  if (subagents.some((item) => ACTIVE_SUBAGENT_STATUSES.has(item.status))) {
    return 'running';
  }
  if (subagents.some((item) => item.failed)) {
    return 'failed';
  }

  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    const payload = resolveEventPayload(event);
    const eventName = resolveEventName(event);
    if (eventName === 'error') {
      return 'failed';
    }
    if (eventName === 'turn_terminal') {
      const status = normalizeText(payload.status).toLowerCase();
      if (status === 'completed') return 'completed';
      if (status === 'rejected' || status === 'failed' || status === 'error') return 'failed';
      if (normalizeText(payload.stop_reason).toUpperCase() === 'USER_BUSY') return 'failed';
    }
    if (eventName === 'final') {
      return 'completed';
    }
  }

  if (subagents.length > 0) {
    return 'completed';
  }
  return 'idle';
};

export const useBeeroomDispatchSessionPreview = (options: {
  sessionId: Ref<string>;
  targetAgentId: Ref<string>;
  targetName: Ref<string>;
  runtimeStatus: Ref<DispatchRuntimeStatus>;
  clearedAfter: Ref<number>;
}) => {
  const rawPreview = ref<BeeroomDispatchSessionPreview | null>(null);

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
      rawPreview.value = null;
      cancelActiveRequest();
      clearSyncTimer();
      return;
    }

    cancelActiveRequest();
    const controller = new AbortController();
    activeController = controller;

    try {
      const sessionRequest =
        !requestedTargetAgentId || !requestedTargetName
          ? getSession(sessionId).catch(() => null)
          : Promise.resolve(null);
      const [sessionResponse, eventsResponse, subagentsResponse] = await Promise.all([
        sessionRequest,
        getSessionEvents(sessionId, { signal: controller.signal }),
        getSessionSubagents(
          sessionId,
          { limit: SUBAGENT_LIST_LIMIT },
          { signal: controller.signal }
        )
      ]);
      if (controller.signal.aborted) return;

      const eventsPayload = eventsResponse?.data?.data || {};
      const sessionDetail = sessionResponse?.data?.data || null;
      const events = flattenRounds(eventsPayload.rounds);
      const running = eventsPayload.running === true;
      const subagents = (Array.isArray(subagentsResponse?.data?.data?.items)
        ? subagentsResponse.data.data.items
        : []
      )
        .map((item: unknown) => normalizeBeeroomMissionSubagentItem(item))
        .filter(
          (item: BeeroomMissionSubagentItem | null): item is BeeroomMissionSubagentItem => Boolean(item)
        )
        .sort((left, right) => {
          const activeDiff =
            Number(ACTIVE_SUBAGENT_STATUSES.has(right.status)) -
            Number(ACTIVE_SUBAGENT_STATUSES.has(left.status));
          if (activeDiff !== 0) return activeDiff;
          return Number(right.updatedTime || 0) - Number(left.updatedTime || 0);
        });
      const targetAgentId =
        requestedTargetAgentId || normalizeText(sessionDetail?.agent_id ?? '');
      const targetName = requestedTargetName || targetAgentId;

      const summary = resolveSummaryFromEvents(events);
      const updatedTime = Math.max(
        ...events.map(resolveEventTimestamp),
        ...subagents.map((item) => Number(item.updatedTime || 0)),
        running || ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.runtimeStatus.value)
          ? Math.floor(Date.now() / 1000)
          : 0
      );
      rawPreview.value = {
        sessionId,
        targetAgentId,
        targetName,
        status: resolvePreviewStatus(options.runtimeStatus.value, running, events, subagents),
        summary,
        dispatchLabel: resolveDispatchLabel(events, summary),
        updatedTime,
        subagents
      };

      const shouldPoll =
        running ||
        ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.runtimeStatus.value) ||
        subagents.some((item) => ACTIVE_SUBAGENT_STATUSES.has(item.status));
      clearSyncTimer();
      if (shouldPoll) {
        scheduleSync(1250);
      }
    } catch (error) {
      if ((error as { name?: string })?.name === 'CanceledError') return;
      if ((error as { name?: string })?.name === 'AbortError') return;
      rawPreview.value = null;
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
      filteredSubagents.some((item) => ACTIVE_SUBAGENT_STATUSES.has(item.status));
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
    clearSyncTimer();
    cancelActiveRequest();
  });

  return {
    dispatchPreview
  };
};
