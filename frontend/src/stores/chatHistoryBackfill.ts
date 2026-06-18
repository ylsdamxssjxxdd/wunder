export type HistoryBackfillMessage = Record<string, unknown>;

export type HistoryBackfillPage = {
  transcript: HistoryBackfillMessage[];
  hasMore: boolean;
  beforeId: number | null;
};

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

export const normalizeHistoryBeforeId = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const readHistoryBackfillPage = (payload: unknown): HistoryBackfillPage => {
  const record = asRecord(payload) || {};
  const transcript = Array.isArray(record.transcript)
    ? record.transcript.filter((item): item is HistoryBackfillMessage => asRecord(item) !== null)
    : [];
  return {
    transcript,
    hasMore: Boolean(
      record.history_has_more ??
        record.historyHasMore ??
        record.history_more ??
        record.historyMore ??
        false
    ),
    beforeId: normalizeHistoryBeforeId(
      record.history_before_id ??
        record.historyBeforeId ??
        record.history_before ??
        record.historyBefore
    )
  };
};

export const resolveHistoryBackfillMessageId = (message: unknown): number | null => {
  const record = asRecord(message);
  if (!record) return null;
  const parsed = Number.parseInt(String(record.history_id ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const buildExistingHistoryIdSet = (messages: unknown[] | null | undefined): Set<number> => {
  const ids = new Set<number>();
  if (!Array.isArray(messages)) {
    return ids;
  }
  messages.forEach((message) => {
    const id = resolveHistoryBackfillMessageId(message);
    if (id !== null) {
      ids.add(id);
    }
  });
  return ids;
};

export const collectDedupedHistoryBackfillPage = (
  incoming: unknown[] | null | undefined,
  existingIds: Set<number>
): HistoryBackfillMessage[] => {
  if (!Array.isArray(incoming)) {
    return [];
  }
  const output: HistoryBackfillMessage[] = [];
  incoming.forEach((message) => {
    const record = asRecord(message);
    if (!record) return;
    const id = resolveHistoryBackfillMessageId(record);
    if (id !== null) {
      if (existingIds.has(id)) return;
      existingIds.add(id);
    }
    output.push(record);
  });
  return output;
};

export const prependHistoryBackfillPage = (
  accumulated: HistoryBackfillMessage[],
  page: HistoryBackfillMessage[]
): HistoryBackfillMessage[] => {
  if (!page.length) {
    return accumulated;
  }
  // Older pages are discovered after newer duplicate/empty pages; keep final order chronological.
  return [...page, ...accumulated];
};
