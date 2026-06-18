import { buildAssistantMatchEntries } from './chatAssistantMatch';
import { isCompactionMarkerAssistantMessage } from './chatCompactionMarker';
import { normalizeFlag, normalizeStreamRound } from './chatStreamIds';

type ChatMessage = Record<string, any>;

const normalizeText = (value: unknown): string =>
  String(value ?? '')
    .replace(/<think>[\s\S]*?<\/think>/gi, ' ')
    .replace(/\s+/g, ' ')
    .trim();

const hasWorkflowTrace = (message: ChatMessage | null | undefined): boolean =>
  Boolean(Array.isArray(message?.workflowItems) && message.workflowItems.length > 0);

const isAssistantFinalTranscriptDuplicate = (
  left: ChatMessage,
  right: ChatMessage
): boolean => {
  if (!left || !right || left.role !== 'assistant' || right.role !== 'assistant') {
    return false;
  }
  if (left.isGreeting || right.isGreeting) return false;
  if (isCompactionMarkerAssistantMessage(left) || isCompactionMarkerAssistantMessage(right)) {
    return false;
  }

  const leftText = normalizeText(left.content);
  const rightText = normalizeText(right.content);
  if (!leftText || !rightText) return false;
  if (leftText === rightText) return true;

  const shorter = leftText.length <= rightText.length ? leftText : rightText;
  const longer = shorter === leftText ? rightText : leftText;
  return shorter.length >= 80 && longer.includes(shorter);
};

const resolvePreferredAssistant = (left: ChatMessage, right: ChatMessage): ChatMessage => {
  const leftWorkflow = hasWorkflowTrace(left);
  const rightWorkflow = hasWorkflowTrace(right);
  if (leftWorkflow !== rightWorkflow) return leftWorkflow ? left : right;
  const leftScore =
    String(left.content || '').length +
    (normalizeFlag(left.stream_incomplete) ? 0 : 100) +
    (left.stats ? 50 : 0);
  const rightScore =
    String(right.content || '').length +
    (normalizeFlag(right.stream_incomplete) ? 0 : 100) +
    (right.stats ? 50 : 0);
  return rightScore > leftScore ? right : left;
};

const mergeAssistantPair = (left: ChatMessage, right: ChatMessage): ChatMessage => {
  const primary = resolvePreferredAssistant(left, right);
  const secondary = primary === left ? right : left;
  const merged = { ...secondary, ...primary };
  if (String(secondary.content || '').length > String(merged.content || '').length) {
    merged.content = secondary.content;
  }
  if (String(secondary.reasoning || '').length > String(merged.reasoning || '').length) {
    merged.reasoning = secondary.reasoning;
  }
  if (!hasWorkflowTrace(merged) && hasWorkflowTrace(secondary)) {
    merged.workflowItems = secondary.workflowItems;
  }
  if (!merged.stats && secondary.stats) {
    merged.stats = secondary.stats;
  }
  if (!merged.feedback && secondary.feedback) {
    merged.feedback = secondary.feedback;
  }
  if (!merged.history_id && secondary.history_id) {
    merged.history_id = secondary.history_id;
  }
  const leftRound = normalizeStreamRound(left.stream_round);
  const rightRound = normalizeStreamRound(right.stream_round);
  if (leftRound !== null && rightRound !== null && leftRound === rightRound) {
    merged.stream_round = leftRound;
  } else if (normalizeStreamRound(merged.stream_round) === null) {
    merged.stream_round = leftRound ?? rightRound ?? merged.stream_round;
  }
  merged.stream_incomplete = Boolean(left.stream_incomplete && right.stream_incomplete);
  merged.workflowStreaming = Boolean(left.workflowStreaming && right.workflowStreaming);
  merged.reasoningStreaming = Boolean(left.reasoningStreaming && right.reasoningStreaming);
  return merged;
};

export const mergeFinalTranscriptAssistantDuplicates = (
  messages: ChatMessage[] | null | undefined
): ChatMessage[] => {
  if (!Array.isArray(messages) || messages.length < 2) {
    return Array.isArray(messages) ? messages : [];
  }
  const entries = buildAssistantMatchEntries(messages);
  if (entries.length < 2) return messages;

  const replaceMap = new Map<ChatMessage, ChatMessage>();
  const removeSet = new Set<ChatMessage>();
  for (let index = 1; index < entries.length; index += 1) {
    const previous = entries[index - 1];
    const current = entries[index];
    if (
      previous.userTurnIndex !== current.userTurnIndex ||
      previous.assistantTurnIndex + 1 !== current.assistantTurnIndex
    ) {
      continue;
    }
    const left = replaceMap.get(previous.message) || previous.message;
    const right = replaceMap.get(current.message) || current.message;
    if (!isAssistantFinalTranscriptDuplicate(left, right)) continue;
    const merged = mergeAssistantPair(left, right);
    replaceMap.set(previous.message, merged);
    replaceMap.set(current.message, merged);
    removeSet.add(current.message);
  }
  if (!removeSet.size) return messages;
  return messages
    .filter((message) => !removeSet.has(message))
    .map((message) => replaceMap.get(message) || message);
};
