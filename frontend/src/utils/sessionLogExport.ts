import { getSession as getChatSessionApi, getSessionEvents as getChatSessionEventsApi } from '@/api/chat';
import { getCurrentLanguage } from '@/i18n';
import { saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';

type SessionRoundEvent = {
  event?: unknown;
  type?: unknown;
  data?: unknown;
  timestamp?: unknown;
};

type SessionRound = {
  user_round?: unknown;
  round?: unknown;
  events?: SessionRoundEvent[];
};

type SessionDetail = {
  id: string;
  title: string;
  agentId: string;
  agentName: string;
  createdAt: unknown;
  updatedAt: unknown;
  lastMessageAt: unknown;
  messageCount: number;
  historyIncomplete: boolean;
  messages: Record<string, unknown>[];
};

type SessionExportLine = Record<string, unknown>;

type SessionExportBundle = {
  session: SessionDetail;
  rounds: SessionRound[];
  running: boolean;
  lastEventId: number;
};

type ExportEventItem = {
  order: number;
  round: number;
  eventType: string;
  title: string;
  rawEvent: SessionRoundEvent;
};

export type ExportedSessionSource = {
  sessionId: string;
  agentName?: string;
  label?: string;
};

const EVENT_TITLE_MAX_LENGTH = 120;

const normalizeTimestamp = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? 0 : value.getTime();
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return 0;
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return 0;
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  }
  const date = new Date(text);
  return Number.isNaN(date.getTime()) ? 0 : date.getTime();
};

const normalizeRoundIndex = (value: unknown, fallback: number): number => {
  const parsed = Number.parseInt(String(value ?? fallback), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return Math.max(1, fallback);
  }
  return parsed;
};

const unwrapEventData = (payload: unknown): unknown => {
  if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
    return payload;
  }
  const source = payload as Record<string, unknown>;
  const hasSessionId = typeof source.session_id === 'string' && source.session_id.trim().length > 0;
  const hasTimestamp = typeof source.timestamp === 'string' && source.timestamp.trim().length > 0;
  const inner = source.data;
  if (hasSessionId && hasTimestamp && inner && typeof inner === 'object') {
    return inner;
  }
  return payload;
};

const fallbackEventDataText = (value: unknown): string => {
  if (value === null || value === undefined) {
    return '';
  }
  if (typeof value === 'string') {
    return value;
  }
  if (Array.isArray(value)) {
    return `[array(${value.length})]`;
  }
  if (typeof value === 'object') {
    try {
      const source = value as Record<string, unknown>;
      const keys = Object.keys(source);
      if (!keys.length) {
        return '{}';
      }
      const preview: Record<string, unknown> = {};
      keys.slice(0, 8).forEach((key) => {
        const field = source[key];
        if (field === null || field === undefined) {
          preview[key] = field;
          return;
        }
        if (typeof field === 'string' || typeof field === 'number' || typeof field === 'boolean') {
          preview[key] = field;
          return;
        }
        if (typeof field === 'bigint') {
          preview[key] = field.toString();
          return;
        }
        if (Array.isArray(field)) {
          preview[key] = `[array(${field.length})]`;
          return;
        }
        preview[key] = '[object]';
      });
      if (keys.length > 8) {
        preview.__extra_keys__ = keys.length - 8;
      }
      const text = JSON.stringify(preview);
      return typeof text === 'string' ? text : '{...}';
    } catch {
      return '{...}';
    }
  }
  return String(value);
};

const safeStringifyEventData = (value: unknown, pretty = false): string => {
  const seen = new WeakSet<object>();
  try {
    const text = JSON.stringify(
      value ?? null,
      (_key, current: unknown) => {
        if (typeof current === 'bigint') {
          return current.toString();
        }
        if (typeof current === 'function') {
          return `[Function ${current.name || 'anonymous'}]`;
        }
        if (typeof current === 'symbol') {
          return String(current);
        }
        if (current instanceof Error) {
          return {
            name: current.name,
            message: current.message,
            stack: current.stack
          };
        }
        if (current && typeof current === 'object') {
          const objectValue = current as object;
          if (seen.has(objectValue)) {
            return '[Circular]';
          }
          seen.add(objectValue);
          if (current instanceof Map) {
            return Object.fromEntries(current.entries());
          }
          if (current instanceof Set) {
            return Array.from(current.values());
          }
        }
        return current;
      },
      pretty ? 2 : undefined
    );
    return typeof text === 'string' ? text : fallbackEventDataText(value);
  } catch {
    return fallbackEventDataText(value);
  }
};

const stringifyEventData = (payload: unknown, pretty = false): string => {
  const resolved = unwrapEventData(payload);
  if (typeof resolved === 'string') {
    return resolved;
  }
  return safeStringifyEventData(resolved, pretty);
};

const truncateText = (value: unknown): string => {
  const text = String(value || '')
    .replace(/\s+/g, ' ')
    .trim();
  if (!text) {
    return '';
  }
  if (text.length <= EVENT_TITLE_MAX_LENGTH) {
    return text;
  }
  return `${text.slice(0, EVENT_TITLE_MAX_LENGTH)}...`;
};

const extractEventTitleText = (value: unknown, depth = 0): string => {
  if (value === null || value === undefined || depth > 3) {
    return '';
  }
  if (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    typeof value === 'bigint'
  ) {
    return String(value).trim();
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      const text = extractEventTitleText(item, depth + 1);
      if (text) {
        return text;
      }
    }
    return '';
  }
  if (typeof value !== 'object') {
    return '';
  }
  const source = value as Record<string, unknown>;
  for (const key of [
    'summary',
    'message',
    'question',
    'reason',
    'error',
    'tool',
    'tool_name',
    'toolName',
    'name',
    'model',
    'model_name',
    'stage',
    'status',
    'code',
    'title'
  ]) {
    const text = extractEventTitleText(source[key], depth + 1);
    if (text) {
      return text;
    }
  }
  return '';
};

const resolveEventTitle = (eventType: string, payload: unknown): string => {
  const normalizedType = String(eventType || '')
    .trim()
    .toLowerCase();
  const data = unwrapEventData(payload);
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    const source = data as Record<string, unknown>;
    const candidates =
      normalizedType === 'user_input'
        ? [
            source.message,
            source.question,
            source.input,
            source.content,
            source.summary,
            source.error,
            source.reason,
            source.tool,
            source.tool_name,
            source.toolName,
            source.name,
            source.model,
            source.model_name,
            source.stage,
            source.status
          ]
        : [
            source.summary,
            source.message,
            source.question,
            source.error,
            source.reason,
            source.tool,
            source.tool_name,
            source.toolName,
            source.name,
            source.model,
            source.model_name,
            source.stage,
            source.status
          ];
    for (const candidate of candidates) {
      const title = truncateText(extractEventTitleText(candidate));
      if (title) {
        return title;
      }
    }
  }
  if (typeof data === 'string') {
    const title = truncateText(data);
    if (title) {
      return title;
    }
  }
  const fallback = truncateText(stringifyEventData(data, false));
  return fallback || '-';
};

const normalizeSession = (sessionId: string, value: unknown): SessionDetail => {
  const source =
    value && typeof value === 'object' && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  const messages = Array.isArray(source.messages)
    ? source.messages
        .filter((item) => item && typeof item === 'object' && !Array.isArray(item))
        .map((item) => item as Record<string, unknown>)
    : [];
  return {
    id: String(source.id || sessionId),
    title: String(source.title || ''),
    agentId: String(source.agent_id || ''),
    agentName: String(source.agent_name || ''),
    createdAt: source.created_at,
    updatedAt: source.updated_at,
    lastMessageAt: source.last_message_at,
    messageCount: messages.length,
    historyIncomplete: Boolean(source.history_incomplete),
    messages
  };
};

const normalizeRounds = (value: unknown): SessionRound[] => {
  if (!Array.isArray(value)) {
    return [];
  }
  return value
    .map((item) => {
      const source =
        item && typeof item === 'object' && !Array.isArray(item)
          ? (item as Record<string, unknown>)
          : {};
      const events = Array.isArray(source.events)
        ? source.events
            .filter((event) => event && typeof event === 'object' && !Array.isArray(event))
            .map((event) => event as SessionRoundEvent)
        : [];
      return {
        user_round: source.user_round,
        round: source.round,
        events
      } as SessionRound;
    })
    .filter((item) => Array.isArray(item.events) && item.events.length > 0);
};

const isDefaultHiddenEventType = (eventType: string): boolean => {
  const normalized = String(eventType || '')
    .trim()
    .toLowerCase();
  return normalized.endsWith('_delta') || normalized === 'context_usage';
};

const normalizeExportTimestamp = (value: unknown): string => {
  const text = String(value || '').trim();
  if (text) {
    const parsed = new Date(text);
    if (!Number.isNaN(parsed.getTime())) {
      return parsed.toISOString();
    }
  }
  const timestampMs = normalizeTimestamp(value);
  if (timestampMs > 0) {
    return new Date(timestampMs).toISOString();
  }
  return '';
};

const sanitizeFilenamePart = (value: unknown, fallback: string): string => {
  const text = String(value || '')
    .trim()
    .replace(/[\\/:*?"<>|]+/g, '_')
    .replace(/\s+/g, '_');
  return text || fallback;
};

const resolvePrimaryQuestion = (session: SessionDetail | null): string => {
  if (!session) return '';
  for (const message of session.messages) {
    if (String(message?.role || '').trim() !== 'user') continue;
    const content = String(message?.content || '').trim();
    if (content) {
      return content;
    }
  }
  return String(session.title || '').trim();
};

const buildEventItems = (rounds: SessionRound[]): ExportEventItem[] => {
  const result: ExportEventItem[] = [];
  let order = 0;
  rounds.forEach((round, roundIndex) => {
    const roundIndexValue = normalizeRoundIndex(round?.user_round ?? round?.round, roundIndex + 1);
    const eventList = Array.isArray(round?.events) ? round.events : [];
    eventList.forEach((event) => {
      order += 1;
      const eventType = String(event?.event || event?.type || 'unknown').trim() || 'unknown';
      result.push({
        order,
        round: roundIndexValue,
        eventType,
        title: resolveEventTitle(eventType, event?.data),
        rawEvent: event
      });
    });
  });
  return result;
};

const buildEventSummary = (eventType: string, payload: unknown): Record<string, unknown> => {
  const data = unwrapEventData(payload);
  if (!data || typeof data !== 'object' || Array.isArray(data)) {
    return {};
  }
  const source = data as Record<string, unknown>;
  const summary: Record<string, unknown> = {};
  for (const key of [
    'stage',
    'summary',
    'message',
    'question',
    'trace_id',
    'model_round',
    'tool',
    'tool_name',
    'stop_reason',
    'ok'
  ]) {
    const value = source[key];
    if (value !== undefined && value !== null && String(value).trim() !== '') {
      summary[key] = value;
    }
  }
  if (eventType === 'tool_call' && source.args && typeof source.args === 'object') {
    summary.args = source.args;
  }
  if (eventType === 'tool_result' && source.meta && typeof source.meta === 'object') {
    summary.meta = source.meta;
  }
  if (eventType === 'llm_output' && source.usage && typeof source.usage === 'object') {
    summary.usage = source.usage;
  }
  return summary;
};

const buildSessionExportLines = (
  bundle: SessionExportBundle,
  options: { includeHiddenEvents?: boolean; exportLabel?: string } = {}
): SessionExportLine[] => {
  const eventItems = buildEventItems(bundle.rounds).filter((item) =>
    options.includeHiddenEvents ? true : !isDefaultHiddenEventType(item.eventType)
  );
  const uniqueEventTypes = new Set<string>();
  const output = eventItems.map<SessionExportLine>((item, index) => {
    uniqueEventTypes.add(item.eventType);
    return {
      record_type: 'event',
      order: index + 1,
      round: item.round,
      event: item.eventType,
      timestamp: normalizeExportTimestamp(item.rawEvent?.timestamp),
      timestamp_ms: normalizeTimestamp(item.rawEvent?.timestamp),
      title: item.title,
      summary: buildEventSummary(item.eventType, item.rawEvent?.data),
      data: unwrapEventData(item.rawEvent?.data)
    };
  });
  output.unshift({
    record_type: 'meta',
    export_schema_version: 4,
    export_format: 'jsonl',
    exported_at: new Date().toISOString(),
    export_label: String(options.exportLabel || '').trim(),
    summary: {
      question: resolvePrimaryQuestion(bundle.session),
      round_count: bundle.rounds.length,
      event_count: output.length,
      event_types: Array.from(uniqueEventTypes).sort((left, right) => left.localeCompare(right)),
      running: bundle.running,
      last_event_id: bundle.lastEventId
    },
    session: bundle.session
  });
  return output;
};

const buildSessionExportFilename = (
  session: SessionDetail,
  options: { prefix?: string; sessionIdFallback?: string } = {}
): string => {
  const prefix = sanitizeFilenamePart(
    options.prefix || session.agentName || session.title,
    sanitizeFilenamePart(options.sessionIdFallback || session.id, 'session')
  );
  const safeSessionId = sanitizeFilenamePart(options.sessionIdFallback || session.id, 'session');
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  return `${prefix}-${safeSessionId}-${timestamp}.jsonl`;
};

const buildBundleExportFilename = (sources: ExportedSessionSource[], prefix?: string): string => {
  const primary = sanitizeFilenamePart(
    prefix || sources.find((item) => String(item.agentName || '').trim())?.agentName || 'orchestration-logs',
    'orchestration-logs'
  );
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
  return `${primary}-${timestamp}.jsonl`;
};

const fetchSessionExportBundle = async (sessionId: string): Promise<SessionExportBundle> => {
  const targetId = String(sessionId || '').trim();
  const [sessionRes, eventsRes] = await Promise.all([
    getChatSessionApi(targetId),
    getChatSessionEventsApi(targetId).catch(() => null)
  ]);
  const sessionData = (sessionRes?.data as { data?: unknown } | undefined)?.data;
  const eventPayload = (eventsRes?.data as { data?: Record<string, unknown> } | undefined)?.data;
  const parsedLastEventId = Number.parseInt(String(eventPayload?.last_event_id ?? 0), 10);
  return {
    session: normalizeSession(targetId, sessionData),
    rounds: normalizeRounds(eventPayload?.rounds),
    running: Boolean(eventPayload?.running),
    lastEventId: Number.isFinite(parsedLastEventId) && parsedLastEventId > 0 ? parsedLastEventId : 0
  };
};

const serializeLines = (lines: SessionExportLine[]): string =>
  lines.map((item) => JSON.stringify(item)).join('\n');

const downloadTextAsFile = (payload: string, filename: string) => {
  const blob = new Blob([payload], {
    type: 'application/x-ndjson;charset=utf-8'
  });
  const objectUrl = URL.createObjectURL(blob);
  saveObjectUrlAsFile(objectUrl, filename);
  window.setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
};

export const exportSingleSessionLog = async (
  sessionId: string,
  options: { filenamePrefix?: string } = {}
) => {
  const bundle = await fetchSessionExportBundle(sessionId);
  const lines = buildSessionExportLines(bundle);
  const filename = buildSessionExportFilename(bundle.session, {
    prefix: options.filenamePrefix,
    sessionIdFallback: sessionId
  });
  downloadTextAsFile(serializeLines(lines), filename);
  return {
    filename,
    session: bundle.session
  };
};

export const downloadSessionLogLines = (
  lines: SessionExportLine[],
  options: {
    sessionId: string;
    agentName?: string;
    title?: string;
    filenamePrefix?: string;
  }
) => {
  const session = normalizeSession(options.sessionId, {
    id: options.sessionId,
    agent_id: '',
    agent_name: options.agentName || '',
    title: options.title || '',
    messages: []
  });
  const filename = buildSessionExportFilename(session, {
    prefix: options.filenamePrefix || options.agentName || options.title || options.sessionId,
    sessionIdFallback: options.sessionId
  });
  downloadTextAsFile(serializeLines(lines), filename);
  return filename;
};

export const exportMultipleSessionLogs = async (
  sources: ExportedSessionSource[],
  options: { filenamePrefix?: string } = {}
) => {
  const normalizedSources = sources
    .map((item) => ({
      sessionId: String(item.sessionId || '').trim(),
      agentName: String(item.agentName || '').trim(),
      label: String(item.label || '').trim()
    }))
    .filter((item) => item.sessionId);
  const uniqueSources = normalizedSources.filter(
    (item, index, array) => array.findIndex((candidate) => candidate.sessionId === item.sessionId) === index
  );
  const bundles = await Promise.all(
    uniqueSources.map(async (source) => ({
      source,
      bundle: await fetchSessionExportBundle(source.sessionId)
    }))
  );
  const header: SessionExportLine = {
    record_type: 'bundle_meta',
    export_schema_version: 4,
    export_format: 'jsonl',
    exported_at: new Date().toISOString(),
    exported_from: 'orchestration',
    source_count: bundles.length,
    language: getCurrentLanguage(),
    sessions: bundles.map(({ source, bundle }) => ({
      session_id: bundle.session.id,
      agent_name: bundle.session.agentName || source.agentName || '',
      label: source.label || '',
      round_count: bundle.rounds.length,
      event_count: buildEventItems(bundle.rounds).filter((item) => !isDefaultHiddenEventType(item.eventType)).length
    }))
  };
  const lines: SessionExportLine[] = [header];
  bundles.forEach(({ source, bundle }) => {
    const sessionLines = buildSessionExportLines(bundle, {
      exportLabel: source.label
    }).map((line) => ({
      ...line,
      source_session_id: bundle.session.id,
      source_agent_name: bundle.session.agentName || source.agentName || '',
      source_label: source.label || ''
    }));
    lines.push(...sessionLines);
  });
  const filename = buildBundleExportFilename(uniqueSources, options.filenamePrefix);
  downloadTextAsFile(serializeLines(lines), filename);
  return {
    filename,
    count: bundles.length
  };
};
