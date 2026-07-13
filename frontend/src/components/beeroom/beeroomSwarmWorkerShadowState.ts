type HistoryMessageRecord = {
  role?: unknown;
  content?: unknown;
  hiddenInternal?: unknown;
  isGreeting?: unknown;
};

type SwarmWorkerWorkflowItem = {
  status?: string | null | undefined;
  eventType?: string | null | undefined;
};

type SwarmWorkerEventRecord = {
  event?: unknown;
  type?: unknown;
  data?: unknown;
  timestamp?: unknown;
  timestamp_ms?: unknown;
};

const ACTIVE_WORKER_STATUSES = new Set(['accepted', 'queued', 'pending', 'running', 'waiting', 'resuming', 'merging']);
const ACTIVE_WORKER_EVENT_NAMES = new Set([
  'round_start',
  'received',
  'queued',
  'queue_enter',
  'queue_update',
  'queue_start',
  'llm_request',
  'llm_output_delta',
  'tool_call',
  'tool_call_delta',
  'tool_output',
  'tool_output_delta'
]);

const normalizeText = (value: unknown): string => String(value || '').trim();

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const parseMaybeJsonRecord = (value: unknown): Record<string, unknown> | null => {
  const text = normalizeText(value);
  if (!text) return null;
  try {
    return asRecord(JSON.parse(text));
  } catch {
    return null;
  }
};

const extractReplyTextFromUnknownContent = (value: unknown, depth = 0): string => {
  if (depth > 4 || value === null || value === undefined) return '';
  if (typeof value === 'string') {
    const direct = value.trim();
    if (!direct) return '';
    const parsed = parseMaybeJsonRecord(direct);
    if (parsed) {
      const structured = extractReplyTextFromUnknownContent(parsed, depth + 1);
      if (structured) return structured;
    }
    return direct;
  }
  if (typeof value === 'number' || typeof value === 'boolean') {
    return String(value).trim();
  }
  if (Array.isArray(value)) {
    const parts = value
      .map((item) => extractReplyTextFromUnknownContent(item, depth + 1))
      .filter(Boolean);
    return parts.join('\n').trim();
  }
  const record = asRecord(value);
  if (!record) return '';
  const directCandidates = [
    record.answer,
    record.content,
    record.message,
    record.reply,
    record.output,
    record.text,
    record.final_reply,
    record.finalReply,
    record.visible_text,
    record.visibleText,
    record.body,
    record.value
  ];
  for (const candidate of directCandidates) {
    const resolved = extractReplyTextFromUnknownContent(candidate, depth + 1);
    if (resolved) return resolved;
  }
  const textParts = ['text', 'content', 'message', 'answer', 'output']
    .map((key) => extractReplyTextFromUnknownContent(record[key], depth + 1))
    .filter(Boolean);
  if (textParts.length) {
    return textParts.join('\n').trim();
  }
  return '';
};

const resolveEventName = (event: SwarmWorkerEventRecord): string =>
  normalizeText(event?.event ?? event?.type).toLowerCase();

const resolveEventPayload = (event: SwarmWorkerEventRecord): Record<string, unknown> => {
  const source =
    event?.data && typeof event.data === 'object' && !Array.isArray(event.data)
      ? (event.data as Record<string, unknown>)
      : null;
  return source || {};
};

const normalizeEventMoment = (value: unknown): number => {
  const numeric = Number(value);
  if (Number.isFinite(numeric) && numeric > 0) {
    return numeric > 1_000_000_000_000 ? numeric / 1000 : numeric;
  }
  const parsed = Date.parse(normalizeText(value));
  return Number.isFinite(parsed) ? parsed / 1000 : 0;
};

const resolveLatestEventMoment = (events: SwarmWorkerEventRecord[]): number =>
  events.reduce((latest, event) => {
    const payload = resolveEventPayload(event);
    return Math.max(
      latest,
      normalizeEventMoment(event.timestamp_ms),
      normalizeEventMoment(event.timestamp),
      normalizeEventMoment(payload.timestamp_ms),
      normalizeEventMoment(payload.timestamp),
      normalizeEventMoment(payload.updated_time),
      normalizeEventMoment(payload.updated_at)
    );
  }, 0);

const resolveTerminalStatusFromEvents = (events: SwarmWorkerEventRecord[]): string => {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    const eventName = resolveEventName(event);
    const payload = resolveEventPayload(event);
    const status = normalizeText(payload.status).toLowerCase();
    // Session histories span multiple worker invocations. A later active event
    // makes an earlier final/terminal record irrelevant to the current task.
    if (ACTIVE_WORKER_EVENT_NAMES.has(eventName) || ACTIVE_WORKER_STATUSES.has(status)) {
      return '';
    }
    if (eventName === 'turn_terminal') {
      if (status === 'completed' || status === 'success' || status === 'idle') return 'completed';
      if (status === 'cancelled' || status === 'stopped') return 'cancelled';
      if (status === 'rejected' || status === 'failed' || status === 'error' || status === 'timeout') {
        return 'failed';
      }
      if (normalizeText(payload.stop_reason).toUpperCase() === 'USER_BUSY') return 'failed';
    }
    if (eventName === 'final') {
      return 'completed';
    }
    if (eventName === 'thread_closed') {
      const lastStatus = normalizeText(payload.last_status ?? payload.status).toLowerCase();
      if (lastStatus === 'completed' || lastStatus === 'success' || lastStatus === 'idle') return 'completed';
      if (lastStatus === 'cancelled' || lastStatus === 'stopped') return 'cancelled';
      if (lastStatus === 'failed' || lastStatus === 'error' || lastStatus === 'timeout') return 'failed';
    }
    if (eventName === 'error') {
      return 'failed';
    }
  }
  return '';
};

export const resolveBeeroomSwarmWorkerReplyFromHistoryMessages = (
  messages: HistoryMessageRecord[]
): string => {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (!message || typeof message !== 'object') continue;
    if (message.isGreeting === true || message.hiddenInternal === true) continue;
    if (normalizeText(message.role).toLowerCase() !== 'assistant') continue;
    const content = extractReplyTextFromUnknownContent(message.content);
    if (content) return content;
  }
  return '';
};

export const resolveBeeroomSwarmWorkerTerminalState = (options: {
  currentStatus: string;
  currentUpdatedTime?: number;
  running: boolean;
  events: SwarmWorkerEventRecord[];
  workflowItems: SwarmWorkerWorkflowItem[];
}) => {
  if (options.running) {
    return {
      status: 'running',
      terminal: false,
      failed: false
    };
  }
  const terminalStatusFromEvents = resolveTerminalStatusFromEvents(options.events);
  const currentStatusActive = ACTIVE_WORKER_STATUSES.has(normalizeText(options.currentStatus).toLowerCase());
  const currentUpdatedTime = normalizeEventMoment(options.currentUpdatedTime);
  const latestEventTime = resolveLatestEventMoment(options.events);
  if (currentStatusActive && (!latestEventTime || currentUpdatedTime > latestEventTime)) {
    return {
      status: 'running',
      terminal: false,
      failed: false
    };
  }
  const tailWorkflowItem = options.workflowItems[options.workflowItems.length - 1] || null;
  const tailStatus = normalizeText(tailWorkflowItem?.status).toLowerCase();
  const workflowStillActive = tailStatus === 'loading' || tailStatus === 'pending';
  if (workflowStillActive) {
    return {
      status: 'running',
      terminal: false,
      failed: false
    };
  }
  if (terminalStatusFromEvents === 'completed') {
    return {
      status: 'completed',
      terminal: true,
      failed: false
    };
  }
  if (terminalStatusFromEvents === 'cancelled') {
    return {
      status: 'cancelled',
      terminal: true,
      failed: false
    };
  }
  if (terminalStatusFromEvents === 'failed') {
    return {
      status: 'failed',
      terminal: true,
      failed: true
    };
  }
  if (currentStatusActive) {
    return {
      status: 'running',
      terminal: false,
      failed: false
    };
  }
  const hasFailedWorkflowItem = options.workflowItems.some(
    (item) => normalizeText(item?.status).toLowerCase() === 'failed'
  );
  if (hasFailedWorkflowItem) {
    return {
      status: 'failed',
      terminal: true,
      failed: true
    };
  }
  if (tailStatus === 'completed') {
    return {
      status: 'completed',
      terminal: true,
      failed: false
    };
  }
  const currentStatus = normalizeText(options.currentStatus).toLowerCase();
  if (currentStatus === 'failed' || currentStatus === 'error' || currentStatus === 'timeout') {
    return {
      status: 'failed',
      terminal: true,
      failed: true
    };
  }
  return {
    status: currentStatus || 'completed',
    terminal: currentStatus === 'completed' || currentStatus === 'success' || currentStatus === 'cancelled',
    failed: currentStatus === 'failed' || currentStatus === 'error' || currentStatus === 'timeout'
  };
};
