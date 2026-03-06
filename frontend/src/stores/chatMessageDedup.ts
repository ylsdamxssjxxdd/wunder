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

const hasWorkflowItems = (message: ChatMessage): boolean =>
  Array.isArray(message?.workflowItems) && message.workflowItems.length > 0;

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
  if (!leftText || !rightText) {
    return leftRound !== null && rightRound !== null && leftRound === rightRound;
  }
  if (leftText === rightText) {
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
  return false;
};

const mergeAssistantPair = (left: ChatMessage, right: ChatMessage): ChatMessage => {
  const primary = resolveAssistantScore(right) > resolveAssistantScore(left) ? right : left;
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
