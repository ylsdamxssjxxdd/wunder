import type { ChatRuntimeEvent } from './chatRuntimeTypes';
import {
  buildCanonicalChatRuntimeEvents,
  buildCanonicalClientMessageSubmittedEvent
} from './chatCanonicalEvents';

type BuildStreamRuntimeEventsOptions = {
  sessionId: string;
  eventType: string;
  payload?: Record<string, unknown> | null;
  eventId?: string | number | null;
  requestId?: string | null;
  clientMessageId?: string | null;
  userTurnId?: string | null;
  modelTurnId?: string | null;
  assistantMessageId?: string | null;
  phase?: string | null;
  source?: string | null;
};

type BuildSessionEventsSnapshotOptions = {
  sessionId: string;
  payload?: Record<string, unknown> | null;
  phase?: string | null;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};

const normalizeId = (value: unknown): string => String(value ?? '').trim();

const firstId = (...values: unknown[]): string => {
  for (const value of values) {
    const text = normalizeId(value);
    if (text) return text;
  }
  return '';
};

const normalizeSeq = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const readArray = (value: unknown): Record<string, unknown>[] =>
  Array.isArray(value)
    ? value.map(asRecord).filter((item) => Object.keys(item).length > 0)
    : [];

const readData = (payload: Record<string, unknown>): Record<string, unknown> => {
  const nested = asRecord(payload.data);
  return Object.keys(nested).length > 0 ? nested : payload;
};

const readSegments = (source: Record<string, unknown>): Record<string, unknown>[] => {
  const direct = readArray(source.segments);
  if (direct.length > 0) return direct;
  return readArray(asRecord(source.data).segments);
};

const patchSnapshotEventPayload = (
  payload: Record<string, unknown>,
  source: Record<string, unknown>,
  roundHint: unknown
): Record<string, unknown> => {
  const next = { ...payload };
  const timestamp = firstId(next.timestamp, source.timestamp);
  if (timestamp) {
    next.timestamp = timestamp;
  }
  const eventSeq = normalizeSeq(
    next.event_seq ??
      next.eventSeq ??
      next.event_id ??
      next.eventId ??
      source.event_seq ??
      source.eventSeq ??
      source.event_id ??
      source.eventId ??
      source.id
  );
  if (eventSeq !== null) {
    next.event_seq = eventSeq;
  }
  if (normalizeSeq(next.user_round ?? next.userRound ?? next.round) === null) {
    const roundSeq = normalizeSeq(roundHint);
    if (roundSeq !== null) {
      next.user_round = roundSeq;
    }
  }
  return next;
};

const buildCanonicalSnapshotDeltaSegmentEvents = (
  sessionId: string,
  payload: Record<string, unknown>,
  source: Record<string, unknown>,
  phase: string,
  roundHint?: unknown
): ChatRuntimeEvent[] => {
  const segments = readSegments(payload);
  if (segments.length === 0) return [];
  const parentData = readData(payload);
  return segments.flatMap((segment) => {
    const segmentEventId = firstId(segment.event_id, segment.eventId);
    const segmentPayload = patchSnapshotEventPayload(
      {
        ...parentData,
        ...segment,
        event_id: segmentEventId || parentData.event_id || source.event_id,
        event_seq: segmentEventId || parentData.event_seq || source.event_seq
      },
      source,
      roundHint
    );
    delete segmentPayload.segments;
    return buildCanonicalChatRuntimeEvents({
      sessionId,
      eventType: 'llm_output_delta',
      payload: segmentPayload,
      eventId: segmentEventId || firstId(source.event_id, source.eventId),
      phase,
      source: 'snapshot'
    });
  });
};

const buildCanonicalSnapshotRecordEvents = (
  sessionId: string,
  record: Record<string, unknown>,
  phase: string,
  roundHint?: unknown
): ChatRuntimeEvent[] => {
  const eventType = firstId(record.event_type, record.eventType, record.event, record.type);
  if (!eventType) return [];
  const eventId = firstId(record.event_id, record.eventId, record.event_seq, record.eventSeq, record.id);
  const data = Object.prototype.hasOwnProperty.call(record, 'data')
    ? asRecord(record.data)
    : record;
  if (eventType === 'llm_output_delta' || eventType === 'delta') {
    const segmented = buildCanonicalSnapshotDeltaSegmentEvents(sessionId, data, record, phase, roundHint);
    if (segmented.length > 0) return segmented;
  }
  const payload = patchSnapshotEventPayload(data, record, roundHint);
  return buildCanonicalChatRuntimeEvents({
    sessionId,
    eventType,
    payload,
    eventId: eventId || null,
    phase,
    source: 'snapshot'
  });
};

const buildCanonicalSnapshotRuntimeEvents = (
  sessionId: string,
  payload: Record<string, unknown>,
  phase: string
): ChatRuntimeEvent[] => {
  const runtime = asRecord(payload.runtime);
  const runtimeStatus = firstId(
    runtime.thread_status,
    runtime.threadStatus,
    runtime.status,
    payload.thread_status,
    payload.threadStatus,
    payload.status,
    payload.running === true ? 'running' : payload.running === false ? 'idle' : ''
  );
  if (!runtimeStatus) return [];
  const lastEventId = firstId(payload.last_event_id, payload.lastEventId);
  return buildCanonicalChatRuntimeEvents({
    sessionId,
    eventType: 'thread_status',
    payload: {
      ...runtime,
      status: runtimeStatus,
      thread_status: runtimeStatus
    },
    eventId: lastEventId ? `snapshot:${lastEventId}:runtime` : null,
    phase,
    source: 'snapshot'
  });
};

export const buildCanonicalStreamRuntimeEvents = (
  options: BuildStreamRuntimeEventsOptions
): ChatRuntimeEvent[] =>
  buildCanonicalChatRuntimeEvents({
    ...options,
    source: options.source || 'ws'
  });

export const buildCanonicalSessionEventsSnapshot = (
  options: BuildSessionEventsSnapshotOptions
): ChatRuntimeEvent[] => {
  const sessionId = normalizeId(options.sessionId);
  if (!sessionId) return [];
  const payload = asRecord(options.payload);
  const phase = normalizeId(options.phase) || 'snapshot';
  const rawEvents = readArray(payload.events);
  const events: ChatRuntimeEvent[] = [];

  if (rawEvents.length > 0) {
    rawEvents.forEach((record) => {
      events.push(...buildCanonicalSnapshotRecordEvents(sessionId, record, phase));
    });
  } else {
    readArray(payload.rounds).forEach((round) => {
      const roundHint = round.user_round ?? round.userRound ?? round.round;
      readArray(round.events).forEach((record) => {
        events.push(...buildCanonicalSnapshotRecordEvents(sessionId, record, phase, roundHint));
      });
    });
  }

  events.push(...buildCanonicalSnapshotRuntimeEvents(sessionId, payload, phase));
  return events;
};

export { buildCanonicalClientMessageSubmittedEvent };
