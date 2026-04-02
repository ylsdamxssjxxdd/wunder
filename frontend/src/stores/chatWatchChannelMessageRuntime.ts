type ChatWatchMessage = Record<string, any>;

export type ChatWatchChannelMessageRuntimeOptions = {
  messages: ChatWatchMessage[];
  lastEventId: number;
  eventId: unknown;
  eventTimestampMs: number | null;
  payload: Record<string, any> | null | undefined;
  data: Record<string, any> | null | undefined;
  normalizeEventId: (value: unknown) => number | null;
  buildMessage: (
    role: 'user' | 'assistant',
    content: string,
    createdAt?: string,
    meta?: Record<string, unknown>
  ) => ChatWatchMessage;
  assignStreamEventId: (message: ChatWatchMessage, eventId: unknown) => void;
  insertWatchUserMessage: (
    content: string,
    eventTimestampMs: number | null,
    anchor?: ChatWatchMessage | null,
    options?: Record<string, unknown>
  ) => void;
  clearSupersededPendingAssistantMessages: (messages: ChatWatchMessage[]) => void;
  dismissStaleInquiryPanels: (messages: ChatWatchMessage[]) => void;
  touchUpdatedAt: (timestamp: number) => void;
  notifySnapshot: (immediate?: boolean) => void;
  hiddenInternalUser?: boolean;
  dedupeAssistantWindowMs?: number;
};

export type ChatWatchChannelMessageRuntimeResult = {
  handled: boolean;
  lastEventId: number;
  mutated: boolean;
};

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    if (!normalized) {
      return false;
    }
    return normalized !== 'false' && normalized !== '0' && normalized !== 'no';
  }
  return Boolean(value);
};

const resolveLatestUserIndex = (messages: ChatWatchMessage[]): number => {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.role === 'user') {
      return index;
    }
  }
  return -1;
};

const resolvePendingAssistantMessage = (
  messages: ChatWatchMessage[]
): ChatWatchMessage | null => {
  const latestUserIndex = resolveLatestUserIndex(messages);
  for (let index = messages.length - 1; index > latestUserIndex; index -= 1) {
    const message = messages[index];
    if (message?.role !== 'assistant') {
      continue;
    }
    if (
      normalizeFlag(message.stream_incomplete) ||
      normalizeFlag(message.workflowStreaming) ||
      normalizeFlag(message.reasoningStreaming)
    ) {
      return message;
    }
    return null;
  }
  return null;
};

const resolveCreatedAt = (eventTimestampMs: number | null): string | undefined =>
  Number.isFinite(Number(eventTimestampMs))
    ? new Date(Number(eventTimestampMs)).toISOString()
    : undefined;

const resolvePendingAssistantAnchorIndex = (
  messages: ChatWatchMessage[]
): number => {
  const pendingAssistant = resolvePendingAssistantMessage(messages);
  if (!pendingAssistant) {
    return -1;
  }
  return messages.indexOf(pendingAssistant);
};

const normalizeMessageRole = (payload: Record<string, any> | null | undefined): string =>
  String(payload?.role ?? '')
    .trim()
    .toLowerCase();

const resolveMessageContent = (
  payload: Record<string, any> | null | undefined,
  data: Record<string, any> | null | undefined
): string => String(data?.content ?? payload?.content ?? '').trim();

export const consumeChatWatchChannelMessage = (
  options: ChatWatchChannelMessageRuntimeOptions
): ChatWatchChannelMessageRuntimeResult => {
  const normalizedEventId = options.normalizeEventId(options.eventId);
  let nextLastEventId = Math.max(0, Number(options.lastEventId) || 0);
  if (normalizedEventId !== null) {
    if (normalizedEventId <= nextLastEventId) {
      return { handled: true, lastEventId: nextLastEventId, mutated: false };
    }
    nextLastEventId = normalizedEventId;
  }

  const role = normalizeMessageRole(options.data) || normalizeMessageRole(options.payload);
  const content = resolveMessageContent(options.payload, options.data);
  if ((role !== 'user' && role !== 'assistant') || !content) {
    return { handled: true, lastEventId: nextLastEventId, mutated: false };
  }

  if (role === 'user') {
    if (normalizedEventId === null) {
      options.insertWatchUserMessage(content, options.eventTimestampMs, null, {
        hiddenInternal: options.hiddenInternalUser === true
      });
      return { handled: true, lastEventId: nextLastEventId, mutated: true };
    }

    const duplicateByEventId = options.messages.some(
      (message) =>
        message?.role === 'user' &&
        options.normalizeEventId(message?.stream_event_id) === normalizedEventId
    );
    if (duplicateByEventId) {
      return { handled: true, lastEventId: nextLastEventId, mutated: false };
    }

    const userMessage = options.buildMessage('user', content, resolveCreatedAt(options.eventTimestampMs), {
      hiddenInternal: options.hiddenInternalUser === true
    });
    options.assignStreamEventId(userMessage, options.eventId);
    const anchorIndex = resolvePendingAssistantAnchorIndex(options.messages);
    if (anchorIndex >= 0) {
      options.messages.splice(anchorIndex, 0, userMessage);
    } else {
      options.messages.push(userMessage);
    }
    options.clearSupersededPendingAssistantMessages(options.messages);
    options.dismissStaleInquiryPanels(options.messages);
    options.touchUpdatedAt(options.eventTimestampMs ?? Date.now());
    options.notifySnapshot(true);
    return { handled: true, lastEventId: nextLastEventId, mutated: true };
  }

  if (normalizedEventId !== null) {
    const duplicateByEventId = options.messages.some(
      (message) =>
        message?.role === 'assistant' &&
        options.normalizeEventId(message?.stream_event_id) === normalizedEventId
    );
    if (duplicateByEventId) {
      return { handled: true, lastEventId: nextLastEventId, mutated: false };
    }
  }

  const lastMessage = options.messages[options.messages.length - 1];
  const lastTimestamp = Number(
    lastMessage?.created_at ? Date.parse(String(lastMessage.created_at)) : Number.NaN
  );
  const assistantDedupeWindowMs = Math.max(0, Number(options.dedupeAssistantWindowMs) || 0);
  const duplicateAssistant =
    normalizedEventId === null &&
    lastMessage?.role === 'assistant' &&
    String(lastMessage?.content || '') === content &&
    (
      !Number.isFinite(Number(options.eventTimestampMs)) ||
      !Number.isFinite(lastTimestamp) ||
      Math.abs(Number(options.eventTimestampMs) - lastTimestamp) <= assistantDedupeWindowMs
    );
  if (duplicateAssistant) {
    return { handled: true, lastEventId: nextLastEventId, mutated: false };
  }

  const pendingAssistant = resolvePendingAssistantMessage(options.messages);
  if (pendingAssistant) {
    const previousContent = String(pendingAssistant.content || '');
    const previousEventId = options.normalizeEventId(pendingAssistant.stream_event_id);
    const nextContent = previousContent.trim().length > content.length ? previousContent : content;
    const nextCreatedAt = resolveCreatedAt(options.eventTimestampMs);
    const wasStreaming =
      normalizeFlag(pendingAssistant.stream_incomplete) ||
      normalizeFlag(pendingAssistant.workflowStreaming) ||
      normalizeFlag(pendingAssistant.reasoningStreaming);
    pendingAssistant.content = nextContent;
    if (nextCreatedAt) {
      pendingAssistant.created_at = nextCreatedAt;
    }
    if (normalizedEventId !== null) {
      options.assignStreamEventId(pendingAssistant, options.eventId);
    }
    pendingAssistant.stream_incomplete = false;
    pendingAssistant.workflowStreaming = false;
    pendingAssistant.reasoningStreaming = false;
    options.touchUpdatedAt(options.eventTimestampMs ?? Date.now());
    options.notifySnapshot(true);
    return {
      handled: true,
      lastEventId: nextLastEventId,
      mutated:
        wasStreaming ||
        previousContent !== nextContent ||
        previousEventId !== normalizedEventId
    };
  }

  const assistantMessage = options.buildMessage(
    'assistant',
    content,
    resolveCreatedAt(options.eventTimestampMs)
  );
  options.assignStreamEventId(assistantMessage, options.eventId);
  options.messages.push(assistantMessage);
  options.touchUpdatedAt(options.eventTimestampMs ?? Date.now());
  options.notifySnapshot(true);
  return { handled: true, lastEventId: nextLastEventId, mutated: true };
};
