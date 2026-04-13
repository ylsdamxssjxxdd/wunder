type ChatMessage = Record<string, any>;

export type AssistantMatchEntry = {
  message: ChatMessage;
  userTurnIndex: number;
  assistantTurnIndex: number;
};

const normalizeAssistantMatchContent = (value: unknown): string =>
  String(value || '')
    .replace(/\s+/g, ' ')
    .trim();

export const assistantEntriesShareTurnAnchor = (
  targetEntry: AssistantMatchEntry | null | undefined,
  snapshotEntry: AssistantMatchEntry | null | undefined
): boolean =>
  !targetEntry ||
  !snapshotEntry ||
  (
    targetEntry.userTurnIndex === snapshotEntry.userTurnIndex &&
    targetEntry.assistantTurnIndex === snapshotEntry.assistantTurnIndex
  );

export const findAnchoredAssistantContentMatchIndex = (
  targetEntry: AssistantMatchEntry | null | undefined,
  targetContent: string,
  snapshotEntries: AssistantMatchEntry[] | null | undefined,
  options: { maxIndex?: number; excludedIndices?: Set<number> } = {}
): number => {
  if (!Array.isArray(snapshotEntries) || snapshotEntries.length === 0) {
    return -1;
  }
  const normalizedTarget = normalizeAssistantMatchContent(targetContent);
  if (!normalizedTarget) {
    return -1;
  }
  const maxIndex = Number.isFinite(options.maxIndex)
    ? Math.min(snapshotEntries.length - 1, Math.max(-1, Number(options.maxIndex)))
    : snapshotEntries.length - 1;
  for (let index = maxIndex; index >= 0; index -= 1) {
    if (options.excludedIndices?.has(index)) {
      continue;
    }
    const snapshotEntry = snapshotEntries[index];
    if (!assistantEntriesShareTurnAnchor(targetEntry, snapshotEntry)) {
      continue;
    }
    const snapshotContent = normalizeAssistantMatchContent(snapshotEntry?.message?.content);
    if (
      snapshotContent &&
      (normalizedTarget.includes(snapshotContent) || snapshotContent.includes(normalizedTarget))
    ) {
      return index;
    }
  }
  return -1;
};

// Anchor assistants to the surrounding user turn so repeated model rounds
// from different user turns cannot collide during snapshot hydration.
export const buildAssistantMatchEntries = (
  messages: ChatMessage[] | null | undefined
): AssistantMatchEntry[] => {
  if (!Array.isArray(messages) || messages.length === 0) {
    return [];
  }
  const entries: AssistantMatchEntry[] = [];
  let userTurnIndex = 0;
  let assistantTurnIndex = 0;
  messages.forEach((message) => {
    if (!message || typeof message !== 'object') {
      return;
    }
    if (message.role === 'user' && !message.isGreeting) {
      userTurnIndex += 1;
      assistantTurnIndex = 0;
      return;
    }
    if (message.role !== 'assistant' || message.isGreeting) {
      return;
    }
    entries.push({
      message,
      userTurnIndex,
      assistantTurnIndex
    });
    assistantTurnIndex += 1;
  });
  return entries;
};

export const buildAssistantMatchEntryMap = (
  messages: ChatMessage[] | null | undefined
): Map<ChatMessage, AssistantMatchEntry> => {
  const map = new Map<ChatMessage, AssistantMatchEntry>();
  buildAssistantMatchEntries(messages).forEach((entry) => {
    map.set(entry.message, entry);
  });
  return map;
};
