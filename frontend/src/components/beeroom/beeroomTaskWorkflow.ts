import type { BeeroomMissionTask } from '@/stores/beeroom';

export type BeeroomWorkflowTone = 'pending' | 'loading' | 'completed' | 'failed';

export type BeeroomWorkflowItem = {
  id: string;
  title: string;
  detail: string;
  status: BeeroomWorkflowTone;
  isTool?: boolean;
  eventType?: string;
  toolName?: string;
  toolCallId?: string;
};

export type BeeroomWorkflowPreviewStep = {
  key: string;
  title: string;
  detail: string;
  tone: BeeroomWorkflowTone;
};

export type BeeroomNodeWorkflowLine = {
  key: string;
  main: string;
  detail: string;
  title: string;
};

export type BeeroomTaskWorkflowPreview = {
  badge: string;
  badgeTone: BeeroomWorkflowTone;
  steps: BeeroomWorkflowPreviewStep[];
  fingerprint: string;
};

type SessionEvent = {
  event?: string;
  data?: unknown;
  timestamp?: string;
};

type SessionRound = {
  events?: SessionEvent[];
};

type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

const PREVIEW_STEP_LIMIT = 1;
const PREVIEW_TITLE_LIMIT = 22;
const PREVIEW_DETAIL_LIMIT = 42;
const MIN_WINDOW_PADDING_S = 2;
const ACTIVE_WINDOW_FALLBACK_S = 30;
const TERMINAL_WINDOW_FALLBACK_S = 60;

const TERMINAL_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled']);

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeStatus = (value: unknown): string => normalizeText(value).toLowerCase();

const escapeHtml = (value: unknown): string =>
  String(value || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

const truncateSingleLine = (value: unknown, limit: number): string => {
  const text = normalizeText(value).replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, Math.max(0, limit - 3))}...`;
};

const parseIsoTimestamp = (value: unknown): number => {
  const text = normalizeText(value);
  if (!text) return 0;
  const millis = Date.parse(text);
  return Number.isFinite(millis) ? millis / 1000 : 0;
};

const isTerminalStatus = (value: unknown): boolean => TERMINAL_STATUSES.has(normalizeStatus(value));

const resolveWorkflowTone = (value: unknown): BeeroomWorkflowTone => {
  const status = normalizeStatus(value);
  if (status === 'failed' || status === 'error' || status === 'timeout' || status === 'cancelled') {
    return 'failed';
  }
  if (status === 'running' || status === 'loading' || status === 'executing' || status === 'merging') {
    return 'loading';
  }
  if (status === 'queued' || status === 'pending' || status === 'awaiting_idle') {
    return 'pending';
  }
  return 'completed';
};

const resolveBeeroomStatusLabel = (status: unknown, t: TranslationFn): string => {
  const normalized = normalizeStatus(status);
  if (normalized === 'queued') return t('beeroom.status.queued');
  if (normalized === 'running') return t('beeroom.status.running');
  if (normalized === 'awaiting_idle') return t('beeroom.status.awaitingIdle');
  if (normalized === 'completed' || normalized === 'success') return t('beeroom.status.completed');
  if (normalized === 'failed' || normalized === 'error') return t('beeroom.status.failed');
  if (normalized === 'timeout') return t('beeroom.status.timeout');
  if (normalized === 'cancelled') return t('beeroom.status.cancelled');
  return t('beeroom.status.unknown');
};

const buildDetail = (value: unknown): string => {
  if (typeof value === 'string') return value.trim();
  if (value === null || value === undefined) return '';
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

const resolveToolName = (payload: Record<string, unknown>): string =>
  normalizeText(
    payload.tool ?? payload.name ?? payload.tool_name ?? payload.toolName ?? payload.command_name
  );

const resolveToolCallId = (payload: Record<string, unknown>): string =>
  normalizeText(
    payload.tool_call_id ?? payload.toolCallId ?? payload.call_id ?? payload.callId ?? payload.id
  );

const resolveEventDetail = (eventName: string, payload: Record<string, unknown>, t: TranslationFn): string => {
  const summary = normalizeText(
    payload.result_summary ?? payload.summary ?? payload.message ?? payload.error ?? payload.detail
  );
  if (summary) return summary;
  const stage = normalizeText(payload.stage);
  if (eventName === 'progress' && stage) {
    return t('chat.workflow.stage', { stage });
  }
  const command = normalizeText(payload.command);
  if (command) return command;
  const args = payload.args ?? payload.arguments ?? payload.input;
  const argsText = normalizeText(typeof args === 'string' ? args : buildDetail(args));
  if (argsText) return argsText;
  const content = normalizeText(payload.content ?? payload.answer ?? payload.output);
  if (content) return content;
  return '';
};

const buildFallbackWorkflowItems = (task: BeeroomMissionTask | null, t: TranslationFn): BeeroomWorkflowItem[] => {
  if (!task) {
    return [
      {
        id: 'standby',
        title: t('chat.toolWorkflow.empty'),
        detail: '',
        status: 'pending'
      }
    ];
  }
  const detail = normalizeText(task.result_summary || task.error || '');
  return [
    {
      id: `${task.task_id || 'task'}:status`,
      title: resolveBeeroomStatusLabel(task.status, t),
      detail,
      status: resolveWorkflowTone(task.status)
    }
  ];
};

const flattenWorkflowEvents = (rounds: SessionRound[]): SessionEvent[] => {
  const events: SessionEvent[] = [];
  rounds.forEach((round) => {
    if (!Array.isArray(round?.events)) return;
    round.events.forEach((event) => {
      if (!event || typeof event !== 'object') return;
      events.push(event);
    });
  });
  return events;
};

const filterTaskWindowEvents = (task: BeeroomMissionTask, events: SessionEvent[]): SessionEvent[] => {
  const startedAt = Number(task.started_time || 0);
  const finishedAt = Number(task.finished_time || 0);
  const updatedAt = Number(task.updated_time || 0);
  const fallbackWindow = isTerminalStatus(task.status)
    ? TERMINAL_WINDOW_FALLBACK_S
    : ACTIVE_WINDOW_FALLBACK_S;
  const windowStart = Math.max(0, (startedAt || updatedAt || 0) - Math.max(MIN_WINDOW_PADDING_S, fallbackWindow));
  const windowEnd = finishedAt > 0 ? finishedAt + MIN_WINDOW_PADDING_S : Number.POSITIVE_INFINITY;

  return events.filter((event) => {
    const timestamp = parseIsoTimestamp(event.timestamp);
    if (!timestamp) {
      return windowStart <= 0;
    }
    return timestamp >= windowStart && timestamp <= windowEnd;
  });
};

const mapSessionEventToWorkflowItem = (
  event: SessionEvent,
  index: number,
  t: TranslationFn
): BeeroomWorkflowItem | null => {
  const eventName = normalizeText(event?.event).toLowerCase();
  if (!eventName || eventName === 'heartbeat' || eventName === 'ping') {
    return null;
  }
  const payload = asRecord(event?.data) || {};
  const toolName = resolveToolName(payload);
  const toolCallId = resolveToolCallId(payload);
  const detail = buildDetail(payload);

  if (eventName === 'tool_call') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.toolCall', { tool: toolName || t('chat.workflow.toolUnknown') }),
      detail,
      status: 'loading',
      isTool: true,
      eventType: 'tool_call',
      toolName,
      toolCallId: toolCallId || undefined
    };
  }
  if (eventName === 'tool_result') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.toolResult', { tool: toolName || t('chat.workflow.toolUnknown') }),
      detail,
      status: normalizeText(payload.error) ? 'failed' : 'completed',
      isTool: true,
      eventType: 'tool_result',
      toolName,
      toolCallId: toolCallId || undefined
    };
  }
  if (eventName === 'progress') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.progressUpdate'),
      detail,
      status: 'loading',
      eventType: eventName
    };
  }
  if (eventName === 'llm_request') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.modelRequestSummary'),
      detail,
      status: 'loading',
      eventType: eventName
    };
  }
  if (eventName === 'llm_output') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.modelOutput'),
      detail,
      status: 'completed',
      eventType: eventName
    };
  }
  if (eventName === 'final') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.finalResponse'),
      detail,
      status: 'completed',
      eventType: eventName
    };
  }
  if (eventName === 'error') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.error'),
      detail,
      status: 'failed',
      eventType: eventName
    };
  }
  if (eventName === 'team_task_dispatch') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('beeroom.canvas.legendDispatch'),
      detail,
      status: 'pending',
      eventType: eventName
    };
  }
  if (eventName === 'team_task_update' || eventName === 'team_task_result') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: resolveBeeroomStatusLabel(payload.status, t),
      detail,
      status: resolveWorkflowTone(payload.status),
      eventType: eventName
    };
  }
  if (eventName === 'team_error') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('beeroom.status.failed'),
      detail,
      status: 'failed',
      eventType: eventName
    };
  }
  return {
    id: `workflow:${eventName}:${index}`,
    title: t('chat.workflow.event', { event: eventName }),
    detail,
    status: 'completed',
    eventType: eventName
  };
};

const dedupePreviewSteps = (steps: BeeroomWorkflowPreviewStep[]): BeeroomWorkflowPreviewStep[] => {
  const output: BeeroomWorkflowPreviewStep[] = [];
  steps.forEach((step) => {
    const previous = output[output.length - 1];
    if (previous && previous.title === step.title && previous.detail === step.detail && previous.tone === step.tone) {
      return;
    }
    output.push(step);
  });
  return output;
};

// Parse preview details only when they look like JSON so card rendering stays cheap.
const parsePreviewDetailRecord = (detail: string): Record<string, unknown> | null => {
  const text = normalizeText(detail);
  if (!text) return null;
  try {
    return asRecord(JSON.parse(text));
  } catch {
    return null;
  }
};

function stringifyWorkflowArgValue(value: unknown): string {
  if (typeof value === 'string') return truncateSingleLine(value, 18);
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) {
    return truncateSingleLine(value.map((entry) => stringifyWorkflowArgValue(entry)).filter(Boolean).join(', '), 18);
  }
  if (value && typeof value === 'object') {
    try {
      return truncateSingleLine(JSON.stringify(value), 18);
    } catch {
      return '';
    }
  }
  return '';
}

function summarizeWorkflowArgs(payload: Record<string, unknown> | null): string {
  if (!payload) return '';
  const argSource = payload.args ?? payload.arguments ?? payload.input ?? payload.params ?? payload.parameters ?? null;
  if (typeof argSource === 'string') {
    return truncateSingleLine(argSource, 34);
  }
  if (Array.isArray(argSource)) {
    return truncateSingleLine(argSource.map((entry) => stringifyWorkflowArgValue(entry)).filter(Boolean).join(', '), 34);
  }
  if (argSource && typeof argSource === 'object') {
    const entries = Object.entries(argSource as Record<string, unknown>)
      .filter(([, value]) => value !== null && value !== undefined && stringifyWorkflowArgValue(value))
      .slice(0, 2)
      .map(([key, value]) => `${key}=${stringifyWorkflowArgValue(value)}`);
    if (entries.length) {
      return truncateSingleLine(entries.join(' · '), 38);
    }
  }
  const fallback = normalizeText(
    payload.command ?? payload.path ?? payload.file ?? payload.query ?? payload.prompt ?? payload.message ?? payload.summary
  );
  return truncateSingleLine(fallback, 34);
}

function resolveNodeWorkflowLineParts(item: BeeroomWorkflowItem): { main: string; detail: string } {
  const payload = parsePreviewDetailRecord(item.detail);
  const tool =
    truncateSingleLine(normalizeText(item.toolName || item.title || ''), 24) ||
    truncateSingleLine(item.title, 24);
  const detail = summarizeWorkflowArgs(payload);
  return {
    main: tool,
    detail: truncateSingleLine(detail, 34)
  };
}

function resolveNodeWorkflowLineText(item: BeeroomWorkflowItem): string {
  const parts = resolveNodeWorkflowLineParts(item);
  return truncateSingleLine(parts.detail ? `${parts.main} · ${parts.detail}` : parts.main, 56);
}

const buildPreviewFromItems = (
  task: BeeroomMissionTask | null,
  items: BeeroomWorkflowItem[],
  t: TranslationFn
): BeeroomTaskWorkflowPreview => {
  const source = items.length > 0 ? items : buildFallbackWorkflowItems(task, t);
  const steps = dedupePreviewSteps(
    source
      .map((item, index) => {
        const parsedDetail = parsePreviewDetailRecord(item.detail);
        const detail = truncateSingleLine(
          resolveEventDetail(item.eventType || '', parsedDetail || {}, t) || item.detail,
          PREVIEW_DETAIL_LIMIT
        );
        return {
          key: `${item.id}:${index}`,
          title: truncateSingleLine(item.title, PREVIEW_TITLE_LIMIT),
          detail,
          tone: item.status
        } satisfies BeeroomWorkflowPreviewStep;
      })
      .filter((step) => step.title)
  )
    .slice(-PREVIEW_STEP_LIMIT)
    .reverse();
  const badge = task ? resolveBeeroomStatusLabel(task.status, t) : t('chat.toolWorkflow.empty');
  const badgeTone = task ? resolveWorkflowTone(task.status) : 'pending';
  const fingerprint = [
    badge,
    badgeTone,
    ...(task
      ? [
          task.task_id,
          normalizeText(task.status),
          normalizeText(task.updated_time),
          normalizeText(task.started_time),
          normalizeText(task.finished_time),
          normalizeText(task.result_summary),
          normalizeText(task.error)
        ]
      : ['standby']),
    ...steps.map((step) => `${step.title}:${step.detail}:${step.tone}`)
  ].join('|');
  return {
    badge,
    badgeTone,
    steps,
    fingerprint
  };
};

export const buildTaskWorkflowRuntime = (
  task: BeeroomMissionTask | null,
  rounds: SessionRound[] | null | undefined,
  t: TranslationFn
): { items: BeeroomWorkflowItem[]; preview: BeeroomTaskWorkflowPreview } => {
  if (!task) {
    const items = buildFallbackWorkflowItems(null, t);
    return {
      items,
      preview: buildPreviewFromItems(null, items, t)
    };
  }
  const flattenedEvents = flattenWorkflowEvents(Array.isArray(rounds) ? rounds : []);
  const scopedEvents = filterTaskWindowEvents(task, flattenedEvents);
  const items = scopedEvents
    .map((event, index) => mapSessionEventToWorkflowItem(event, index, t))
    .filter((item): item is BeeroomWorkflowItem => Boolean(item));

  if (!items.length) {
    const fallbackItems = buildFallbackWorkflowItems(task, t);
    return {
      items: fallbackItems,
      preview: buildPreviewFromItems(task, fallbackItems, t)
    };
  }

  return {
    items,
    preview: buildPreviewFromItems(task, items, t)
  };
};

export const buildNodeWorkflowHtml = (
  items: BeeroomWorkflowItem[],
  title: string,
  tone: BeeroomWorkflowTone = 'pending'
): string => {
  const toolItems = items.filter((item) => item.eventType === 'tool_call');
  const lines = toolItems.length
    ? toolItems
        .slice()
        .reverse()
        .map((item) => {
          const parts = resolveNodeWorkflowLineParts(item);
          return `
            <div class="beeroom-node-workflow-step" title="${escapeHtml(resolveNodeWorkflowLineText(item))}">
              <span class="beeroom-node-workflow-step-dot"></span>
              <span class="beeroom-node-workflow-step-text">
                <span class="beeroom-node-workflow-step-main">${escapeHtml(parts.main)}</span>
                ${parts.detail ? `<span class="beeroom-node-workflow-step-detail">${escapeHtml(parts.detail)}</span>` : ''}
              </span>
            </div>
          `;
        })
        .join('')
    : `<div class="beeroom-node-workflow-empty">${escapeHtml(title)}</div>`;
  return `
    <div class="beeroom-node-workflow is-${escapeHtml(tone)} ${toolItems.length ? '' : 'is-empty'}">
      <div class="beeroom-node-workflow-steps">${lines}</div>
    </div>
  `;
};

export const buildNodeWorkflowPreviewLines = (items: BeeroomWorkflowItem[]): BeeroomNodeWorkflowLine[] =>
  items
    .filter((item) => item.eventType === 'tool_call')
    .slice()
    .reverse()
    .map((item, index) => {
      const parts = resolveNodeWorkflowLineParts(item);
      return {
        key: `${item.id || 'tool'}:${index}`,
        main: parts.main,
        detail: parts.detail,
        title: resolveNodeWorkflowLineText(item)
      } satisfies BeeroomNodeWorkflowLine;
    });

