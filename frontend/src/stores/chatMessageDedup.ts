type ChatMessage = Record<string, any>;

const normalizeComparableValue = (value: unknown): string => {
  if (value === null || value === undefined) return '';
  return String(value).trim();
};

const normalizeComparableNumber = (value: unknown): number | null => {
  if (value === null || value === undefined || value === '') return null;
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : null;
};

const resolveTimestampMs = (value: unknown): number | null => {
  const text = normalizeComparableValue(value);
  if (!text) return null;
  const millis = Date.parse(text);
  return Number.isFinite(millis) ? millis : null;
};

const normalizeAssistantText = (value: unknown): string =>
  String(value || '')
    .replace(/<think>[\s\S]*?<\/think>/gi, ' ')
    .replace(/\s+/g, ' ')
    .trim();

const CANCELLATION_MATCH_WINDOW_MS = 30_000;

const CANCELLATION_TOKENS = [
  'session cancelled',
  'session canceled',
  'cancelled',
  'canceled',
  'aborted',
  'stopped by user',
  '会话已取消',
  '请求已中止',
  '请求已终止',
  '已中止',
  '已终止'
];

const ABORT_PREFERRED_TOKENS = ['aborted', '已中止', '已终止', '请求已中止', '请求已终止'];

const SESSION_CANCELLED_TOKENS = ['session cancelled', 'session canceled', '会话已取消'];

const resolveErrorRequestId = (text: string): string => {
  if (!text) return '';
  const matched =
    text.match(/request[_\s-]?id["'\s:=]+([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})/i) ||
    text.match(/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})/i);
  return matched?.[1] ? matched[1].toLowerCase() : '';
};

const isStreamRequestFailureText = (text: string): boolean => {
  const normalized = String(text || '').toLowerCase();
  return (
    normalized.includes('llm stream request failed') ||
    normalized.includes('模型调用失败') ||
    normalized.includes('模型请求失败') ||
    normalized.includes('too many requests') ||
    normalized.includes('quota')
  );
};

const hasWorkflowItems = (message: ChatMessage): boolean =>
  Array.isArray(message?.workflowItems) && message.workflowItems.length > 0;

const collectAssistantComparableTexts = (message: ChatMessage): string[] => {
  if (!message || message.role !== 'assistant') return [];
  const texts = [
    normalizeAssistantText(message.content),
    normalizeAssistantText(message.reasoning),
    normalizeAssistantText(message.stop_reason)
  ];
  const workflowItems = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  workflowItems.forEach((item) => {
    if (!item || typeof item !== 'object') return;
    texts.push(
      normalizeAssistantText(item.title),
      normalizeAssistantText(item.detail),
      normalizeAssistantText(item.error),
      normalizeAssistantText(item.message)
    );
  });
  return texts.filter(Boolean);
};

const isCompactionRelatedAssistantMessage = (message: ChatMessage): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (message.manual_compaction_marker === true || message.manualCompactionMarker === true) {
    return true;
  }
  const workflowItems = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  return workflowItems.some((item) => {
    if (!item || typeof item !== 'object') return false;
    const eventType = normalizeComparableValue(item.eventType || item.event).toLowerCase();
    const toolName = normalizeComparableValue(item.toolName || item.tool || item.name).toLowerCase();
    const toolCallId = normalizeComparableValue(item.toolCallId || item.tool_call_id).toLowerCase();
    const combinedText = normalizeAssistantText(
      `${item.title || ''} ${item.detail || item.message || item.error || ''}`
    ).toLowerCase();
    return (
      eventType.includes('compaction') ||
      toolName.includes('compaction') ||
      toolCallId.startsWith('compaction:') ||
      combinedText.includes('压缩') ||
      combinedText.includes('compaction')
    );
  });
};

const hasCancellationSignal = (message: ChatMessage): boolean => {
  const combined = collectAssistantComparableTexts(message).join(' ').toLowerCase();
  if (!combined) return false;
  return CANCELLATION_TOKENS.some((token) => combined.includes(token));
};

const isMinimalAssistantStatusNotice = (message: ChatMessage): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (String(message.reasoning || '').trim()) return false;
  if (message.plan || message.questionPanel) return false;
  if (Array.isArray(message.subagents) && message.subagents.length > 0) return false;
  const contentLength = normalizeAssistantText(message.content).length;
  const workflowTextLength = (Array.isArray(message.workflowItems) ? message.workflowItems : []).reduce(
    (total, item) =>
      total +
      normalizeAssistantText(`${item?.title || ''} ${item?.detail || item?.message || item?.error || ''}`).length,
    0
  );
  return contentLength <= 32 && workflowTextLength <= 160;
};

const resolveCancellationNoticePriority = (message: ChatMessage): number => {
  const combined = collectAssistantComparableTexts(message).join(' ').toLowerCase();
  if (!combined) return 0;
  if (ABORT_PREFERRED_TOKENS.some((token) => combined.includes(token))) {
    return 3;
  }
  if (SESSION_CANCELLED_TOKENS.some((token) => combined.includes(token))) {
    return 2;
  }
  if (CANCELLATION_TOKENS.some((token) => combined.includes(token))) {
    return 1;
  }
  return 0;
};

const shouldMergeCancellationDuplicatePair = (left: ChatMessage, right: ChatMessage): boolean => {
  if (!left || !right || left.role !== 'assistant' || right.role !== 'assistant') {
    return false;
  }
  if (left.isGreeting || right.isGreeting) {
    return false;
  }
  if (isCompactionRelatedAssistantMessage(left) || isCompactionRelatedAssistantMessage(right)) {
    return false;
  }
  if (!hasCancellationSignal(left) || !hasCancellationSignal(right)) {
    return false;
  }
  if (!isMinimalAssistantStatusNotice(left) && !isMinimalAssistantStatusNotice(right)) {
    return false;
  }
  const leftEventId = normalizeComparableValue(left.stream_event_id);
  const rightEventId = normalizeComparableValue(right.stream_event_id);
  if (leftEventId && rightEventId && leftEventId === rightEventId) {
    return true;
  }
  const leftRound = normalizeComparableNumber(left.stream_round);
  const rightRound = normalizeComparableNumber(right.stream_round);
  if (leftRound !== null && rightRound !== null && leftRound === rightRound) {
    return true;
  }
  const leftTime = resolveTimestampMs(left.created_at);
  const rightTime = resolveTimestampMs(right.created_at);
  return (
    leftTime !== null &&
    rightTime !== null &&
    Math.abs(leftTime - rightTime) <= CANCELLATION_MATCH_WINDOW_MS
  );
};

const resolvePreferredCancellationContent = (left: ChatMessage, right: ChatMessage): string => {
  const candidates = [left, right]
    .filter((message) => String(message?.content || '').trim())
    .sort((a, b) => {
      const aMinimal = isMinimalAssistantStatusNotice(a) ? 1 : 0;
      const bMinimal = isMinimalAssistantStatusNotice(b) ? 1 : 0;
      if (aMinimal !== bMinimal) {
        return aMinimal - bMinimal;
      }
      const priorityDelta = resolveCancellationNoticePriority(b) - resolveCancellationNoticePriority(a);
      if (priorityDelta !== 0) {
        return priorityDelta;
      }
      return String(b?.content || '').trim().length - String(a?.content || '').trim().length;
    });
  return String(candidates[0]?.content || '').trim();
};

const resolveAssistantScore = (message: ChatMessage): number => {
  let score = 0;
  if (!message || message.role !== 'assistant') return score;
  if (message.isGreeting) score -= 10000;
  score += Math.min(String(message.content || '').length, 4000);
  if (hasWorkflowItems(message)) {
    score += 5000 + message.workflowItems.length * 25;
  }
  if (message.reasoning) score += 400;
  if (message.plan) score += 300;
  if (message.questionPanel) score += 300;
  if (!message.stream_incomplete) score += 150;
  if (!message.workflowStreaming) score += 80;
  if (!message.reasoningStreaming) score += 80;
  if (message.stream_event_id !== null && message.stream_event_id !== undefined) score += 100;
  if (message.stream_round !== null && message.stream_round !== undefined) score += 60;
  return score;
};

const shouldDeduplicateAssistantPair = (left: ChatMessage, right: ChatMessage): boolean => {
  if (!left || !right || left.role !== 'assistant' || right.role !== 'assistant') {
    return false;
  }
  if (left.isGreeting || right.isGreeting) {
    return false;
  }
  const leftEventId = normalizeComparableValue(left.stream_event_id);
  const rightEventId = normalizeComparableValue(right.stream_event_id);
  if (leftEventId && rightEventId && leftEventId === rightEventId) {
    return true;
  }
  const leftRound = normalizeComparableNumber(left.stream_round);
  const rightRound = normalizeComparableNumber(right.stream_round);
  const leftText = normalizeAssistantText(left.content);
  const rightText = normalizeAssistantText(right.content);
  const leftRequestId = resolveErrorRequestId(leftText);
  const rightRequestId = resolveErrorRequestId(rightText);
  if (leftRequestId && rightRequestId && leftRequestId === rightRequestId) {
    return true;
  }
  if (!leftText || !rightText) {
    return leftRound !== null && rightRound !== null && leftRound === rightRound;
  }
  if (leftText === rightText) {
    return true;
  }
  if (shouldMergeCancellationDuplicatePair(left, right)) {
    return true;
  }
  const shorter = leftText.length <= rightText.length ? leftText : rightText;
  const longer = shorter === leftText ? rightText : leftText;
  if (shorter.length >= 80 && longer.includes(shorter)) {
    if (leftRound !== null && rightRound !== null && leftRound === rightRound) {
      return true;
    }
    const leftTime = resolveTimestampMs(left.created_at);
    const rightTime = resolveTimestampMs(right.created_at);
    if (leftTime !== null && rightTime !== null && Math.abs(leftTime - rightTime) <= 120000) {
      return true;
    }
  }
  if (
    shorter.length >= 24 &&
    longer.includes(shorter) &&
    isStreamRequestFailureText(leftText) &&
    isStreamRequestFailureText(rightText)
  ) {
    const leftTime = resolveTimestampMs(left.created_at);
    const rightTime = resolveTimestampMs(right.created_at);
    if (leftTime !== null && rightTime !== null && Math.abs(leftTime - rightTime) <= 120000) {
      return true;
    }
  }
  return false;
};

const mergeAssistantPair = (left: ChatMessage, right: ChatMessage): ChatMessage => {
  const cancellationDuplicate = shouldMergeCancellationDuplicatePair(left, right);
  let primary = resolveAssistantScore(right) > resolveAssistantScore(left) ? right : left;
  if (cancellationDuplicate) {
    const leftMinimal = isMinimalAssistantStatusNotice(left);
    const rightMinimal = isMinimalAssistantStatusNotice(right);
    if (leftMinimal !== rightMinimal) {
      primary = leftMinimal ? right : left;
    } else {
      const leftPriority = resolveCancellationNoticePriority(left);
      const rightPriority = resolveCancellationNoticePriority(right);
      if (leftPriority !== rightPriority) {
        primary = rightPriority > leftPriority ? right : left;
      }
    }
  }
  const secondary = primary === left ? right : left;
  const merged: ChatMessage = { ...secondary, ...primary };
  if (String(secondary.content || '').length > String(merged.content || '').length) {
    merged.content = secondary.content;
  }
  if (String(secondary.reasoning || '').length > String(merged.reasoning || '').length) {
    merged.reasoning = secondary.reasoning;
  }
  if (
    (!Array.isArray(merged.workflowItems) || merged.workflowItems.length === 0) &&
    Array.isArray(secondary.workflowItems) &&
    secondary.workflowItems.length > 0
  ) {
    merged.workflowItems = secondary.workflowItems;
  } else if (
    Array.isArray(merged.workflowItems) &&
    Array.isArray(secondary.workflowItems) &&
    secondary.workflowItems.length > merged.workflowItems.length
  ) {
    merged.workflowItems = secondary.workflowItems;
  }
  if (!merged.plan && secondary.plan) {
    merged.plan = secondary.plan;
  }
  if (!merged.questionPanel && secondary.questionPanel) {
    merged.questionPanel = secondary.questionPanel;
  }
  if (
    (merged.stream_event_id === null || merged.stream_event_id === undefined || merged.stream_event_id === '') &&
    secondary.stream_event_id !== null &&
    secondary.stream_event_id !== undefined &&
    secondary.stream_event_id !== ''
  ) {
    merged.stream_event_id = secondary.stream_event_id;
  }
  if (
    (merged.stream_round === null || merged.stream_round === undefined || merged.stream_round === '') &&
    secondary.stream_round !== null &&
    secondary.stream_round !== undefined &&
    secondary.stream_round !== ''
  ) {
    merged.stream_round = secondary.stream_round;
  }
  if (!merged.created_at && secondary.created_at) {
    merged.created_at = secondary.created_at;
  }
  merged.stream_incomplete = Boolean(left.stream_incomplete && right.stream_incomplete);
  merged.workflowStreaming = Boolean(left.workflowStreaming && right.workflowStreaming);
  merged.reasoningStreaming = Boolean(left.reasoningStreaming && right.reasoningStreaming);
  if (cancellationDuplicate) {
    const preferredContent = resolvePreferredCancellationContent(left, right);
    if (preferredContent) {
      merged.content = preferredContent;
    }
  }
  return merged;
};

export const dedupeAssistantMessages = (messages: ChatMessage[] | null | undefined): ChatMessage[] => {
  if (!Array.isArray(messages) || messages.length < 2) {
    return Array.isArray(messages) ? messages : [];
  }
  const result: ChatMessage[] = [];
  let lastAssistantIndex = -1;
  messages.forEach((message) => {
    if (!message) return;
    if (message.role === 'user') {
      result.push(message);
      lastAssistantIndex = -1;
      return;
    }
    if (message.role !== 'assistant') {
      result.push(message);
      return;
    }
    if (lastAssistantIndex >= 0) {
      const current = result[lastAssistantIndex];
      if (shouldDeduplicateAssistantPair(current, message)) {
        result[lastAssistantIndex] = mergeAssistantPair(current, message);
        return;
      }
    }
    result.push(message);
    lastAssistantIndex = result.length - 1;
  });
  return result;
};

export const dedupeAssistantMessagesInPlace = <T extends ChatMessage>(messages: T[] | null | undefined): T[] => {
  if (!Array.isArray(messages) || messages.length < 2) {
    return Array.isArray(messages) ? messages : [];
  }
  const deduped = dedupeAssistantMessages(messages) as T[];
  if (deduped === messages) {
    return messages;
  }
  messages.splice(0, messages.length, ...deduped);
  return messages;
};
