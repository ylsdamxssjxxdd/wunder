type ChatMessage = Record<string, any>;

export type ProtectedRealtimeMessage = {
  eventId: number;
  role: 'user' | 'assistant';
  content: string;
  createdAt?: string;
  hiddenInternal?: boolean;
  trackedAt: number;
};

type ProtectedRealtimeMessageCandidate = {
  eventId?: unknown;
  role?: unknown;
  content?: unknown;
  createdAt?: unknown;
  hiddenInternal?: unknown;
};

type BuildMessage = (
  role: 'user' | 'assistant',
  content: string,
  createdAt?: string,
  meta?: Record<string, unknown>
) => ChatMessage;

type MergeProtectedRealtimeMessagesOptions = {
  messages: ChatMessage[];
  entries: ProtectedRealtimeMessage[];
  normalizeEventId: (value: unknown) => number | null;
  buildMessage: BuildMessage;
  assignStreamEventId: (message: ChatMessage, eventId: unknown) => void;
};

type MergeProtectedRealtimeMessagesResult = {
  mutated: boolean;
  retainedEntries: ProtectedRealtimeMessage[];
};

const MAX_PROTECTED_MESSAGES_PER_SESSION = 24;
const PROTECTED_EVENT_MATCH_WINDOW_MS = 2500;

const normalizeRole = (value: unknown): 'user' | 'assistant' | null => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'user' || normalized === 'assistant') {
    return normalized;
  }
  return null;
};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeComparableText = (value: unknown): string =>
  normalizeText(value).replace(/\s+/g, ' ');

const resolveMessageTimestampMs = (message: ChatMessage | null | undefined): number | null => {
  if (!message) return null;
  const parsed = Date.parse(String(message.created_at ?? ''));
  return Number.isFinite(parsed) ? parsed : null;
};

const isStreamingAssistant = (message: ChatMessage | null | undefined): boolean =>
  Boolean(
    message?.role === 'assistant' &&
      (
        message?.stream_incomplete === true ||
        message?.workflowStreaming === true ||
        message?.reasoningStreaming === true
      )
  );

export const upsertProtectedRealtimeMessage = (
  entries: ProtectedRealtimeMessage[],
  candidate: ProtectedRealtimeMessageCandidate,
  normalizeEventId: (value: unknown) => number | null
): ProtectedRealtimeMessage[] => {
  const eventId = normalizeEventId(candidate?.eventId);
  const role = normalizeRole(candidate?.role);
  const content = normalizeText(candidate?.content);
  if (eventId === null || !role || !content) {
    return Array.isArray(entries) ? entries.slice() : [];
  }
  const nextEntry: ProtectedRealtimeMessage = {
    eventId,
    role,
    content,
    createdAt: typeof candidate?.createdAt === 'string' ? candidate.createdAt : undefined,
    hiddenInternal: candidate?.hiddenInternal === true,
    trackedAt: Date.now()
  };
  const nextEntries = (Array.isArray(entries) ? entries : []).filter((item) => item.eventId !== eventId);
  nextEntries.push(nextEntry);
  nextEntries.sort((left, right) => left.eventId - right.eventId || left.trackedAt - right.trackedAt);
  if (nextEntries.length > MAX_PROTECTED_MESSAGES_PER_SESSION) {
    return nextEntries.slice(nextEntries.length - MAX_PROTECTED_MESSAGES_PER_SESSION);
  }
  return nextEntries;
};

const resolveProtectedInsertIndex = (
  messages: ChatMessage[],
  entry: ProtectedRealtimeMessage,
  normalizeEventId: (value: unknown) => number | null
): number => {
  for (let index = 0; index < messages.length; index += 1) {
    const candidateEventId = normalizeEventId(messages[index]?.stream_event_id);
    if (candidateEventId !== null && candidateEventId > entry.eventId) {
      return index;
    }
  }
  if (entry.role === 'user') {
    for (let index = messages.length - 1; index >= 0; index -= 1) {
      if (isStreamingAssistant(messages[index])) {
        return index;
      }
    }
  }
  return messages.length;
};

const resolveMatchingMessage = (
  messages: ChatMessage[],
  entry: ProtectedRealtimeMessage,
  normalizeEventId: (value: unknown) => number | null
): ChatMessage | null => {
  const normalizedEntryContent = normalizeComparableText(entry.content);
  if (!normalizedEntryContent) {
    return null;
  }
  const entryTimestampMs = entry.createdAt ? Date.parse(entry.createdAt) : Number.NaN;
  const hasEntryTimestamp = Number.isFinite(entryTimestampMs);
  let matched: ChatMessage | null = null;
  let bestDelta = Number.POSITIVE_INFINITY;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const candidate = messages[index];
    if (candidate?.role !== entry.role) continue;
    if (normalizeComparableText(candidate?.content) !== normalizedEntryContent) continue;
    const candidateEventId = normalizeEventId(candidate?.stream_event_id);
    if (candidateEventId !== null && candidateEventId !== entry.eventId) continue;
    if (isStreamingAssistant(candidate)) continue;
    if (!hasEntryTimestamp) {
      matched = candidate;
      break;
    }
    const candidateTimestampMs = resolveMessageTimestampMs(candidate);
    if (!Number.isFinite(candidateTimestampMs)) continue;
    const delta = Math.abs(Number(candidateTimestampMs) - entryTimestampMs);
    if (delta > PROTECTED_EVENT_MATCH_WINDOW_MS) continue;
    if (delta < bestDelta) {
      bestDelta = delta;
      matched = candidate;
      if (delta === 0) break;
    }
  }
  return matched;
};

// Keep channel-side realtime messages visible until chat history returns the same stream event.
export const mergeProtectedRealtimeMessages = (
  options: MergeProtectedRealtimeMessagesOptions
): MergeProtectedRealtimeMessagesResult => {
  const messages = Array.isArray(options.messages) ? options.messages : [];
  const entries = Array.isArray(options.entries) ? options.entries : [];
  if (!entries.length) {
    return { mutated: false, retainedEntries: [] };
  }
  let mutated = false;
  const retainedEntries: ProtectedRealtimeMessage[] = [];
  entries
    .slice()
    .sort((left, right) => left.eventId - right.eventId || left.trackedAt - right.trackedAt)
    .forEach((entry) => {
      const existingByEventId = messages.find(
        (message) =>
          message?.role === entry.role &&
          options.normalizeEventId(message?.stream_event_id) === entry.eventId
      );
      const existing =
        existingByEventId || resolveMatchingMessage(messages, entry, options.normalizeEventId);
      if (existing) {
        if (!existingByEventId) {
          const previousEventId = options.normalizeEventId(existing.stream_event_id);
          const previousHiddenInternal = existing.hiddenInternal === true;
          const previousCreatedAt = String(existing.created_at || '').trim();
          options.assignStreamEventId(existing, entry.eventId);
          if (!previousCreatedAt && entry.createdAt) {
            existing.created_at = entry.createdAt;
          }
          if (entry.role === 'user' && entry.hiddenInternal === true) {
            existing.hiddenInternal = true;
          }
          const nextEventId = options.normalizeEventId(existing.stream_event_id);
          if (
            previousEventId !== nextEventId ||
            (!previousCreatedAt && entry.createdAt) ||
            (!previousHiddenInternal && entry.role === 'user' && entry.hiddenInternal === true)
          ) {
            mutated = true;
          }
        }
        if (existing.realtime_protected === true) {
          retainedEntries.push(entry);
        }
        return;
      }
      const nextMessage = options.buildMessage(entry.role, entry.content, entry.createdAt, {
        hiddenInternal: entry.hiddenInternal === true
      });
      nextMessage.realtime_protected = true;
      options.assignStreamEventId(nextMessage, entry.eventId);
      const insertIndex = resolveProtectedInsertIndex(messages, entry, options.normalizeEventId);
      messages.splice(insertIndex, 0, nextMessage);
      mutated = true;
      retainedEntries.push(entry);
    });
  return {
    mutated,
    retainedEntries
  };
};
