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
      record.has_more ??
        record.hasMore ??
        false
    ),
    beforeId: normalizeHistoryBeforeId(
      record.before_seq ??
        record.beforeSeq
    )
  };
};

export const resolveHistoryBackfillMessageSeq = (message: unknown): number | null => {
  const record = asRecord(message);
  if (!record) return null;
  const parsed = Number.parseInt(String(record.created_seq ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const buildExistingHistoryItemSeqSet = (messages: unknown[] | null | undefined): Set<number> => {
  const ids = new Set<number>();
  if (!Array.isArray(messages)) {
    return ids;
  }
  messages.forEach((message) => {
    const id = resolveHistoryBackfillMessageSeq(message);
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
    const id = resolveHistoryBackfillMessageSeq(record);
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
