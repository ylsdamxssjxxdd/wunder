import type { DispatchRuntimeStatus } from '@/components/beeroom/beeroomCanvasChatModel';

type SessionEventRecord = {
  event?: unknown;
  type?: unknown;
  data?: unknown;
};

type BeeroomDispatchPreviewSubagentLike = {
  status?: unknown;
};

const ACTIVE_BEEROOM_SUBAGENT_STATUSES = new Set(['accepted', 'queued', 'waiting', 'running']);

const ACTIVE_LOCAL_RUNTIME_STATUSES = new Set<DispatchRuntimeStatus>([
  'running',
  'queued',
  'awaiting_approval',
  'resuming'
]);

const normalizeText = (value: unknown): string => String(value || '').trim();

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
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

export const resolveBeeroomTerminalPreviewStatusFromEvents = (events: SessionEventRecord[]): string => {
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
      if (status === 'cancelled' || status === 'stopped') return 'cancelled';
      if (status === 'rejected' || status === 'failed' || status === 'error') return 'failed';
      if (normalizeText(payload.stop_reason).toUpperCase() === 'USER_BUSY') return 'failed';
    }
    if (eventName === 'final') {
      return 'completed';
    }
  }
  return '';
};

export const resolveBeeroomDispatchPreviewStatus = (options: {
  localStatus: DispatchRuntimeStatus;
  running: boolean;
  events: SessionEventRecord[];
  subagents: BeeroomDispatchPreviewSubagentLike[];
}) => {
  if (options.running) return 'running';
  if (options.subagents.some((item) => ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(normalizeText(item.status).toLowerCase()))) {
    return 'running';
  }
  const terminalStatus = resolveBeeroomTerminalPreviewStatusFromEvents(options.events);
  if (terminalStatus) return terminalStatus;
  if (options.localStatus === 'queued') return 'queued';
  if (ACTIVE_LOCAL_RUNTIME_STATUSES.has(options.localStatus)) return 'running';
  if (options.localStatus === 'completed') return 'completed';
  if (options.localStatus === 'failed') return 'failed';
  if (options.localStatus === 'stopped') return 'cancelled';
  return 'idle';
};
