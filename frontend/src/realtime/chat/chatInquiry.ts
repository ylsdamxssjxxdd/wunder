/** Bounded interactive metadata carried by the durable assistant item. */
export interface ChatInquiryOption { label: string; description: string; recommended: boolean }
export interface ChatInquiryQuestion { question: string; multiple: boolean; options: ChatInquiryOption[] }
export interface ChatInquiryPanel {
  questions: ChatInquiryQuestion[];
  keep_open?: boolean;
  keepOpen?: boolean;
  status: 'pending' | 'answered' | 'dismissed';
}

/**
 * Extract the bounded panel payload from either assistant metadata or a
 * question-panel tool result.  Durable thread rows and live events use
 * different envelopes, so this deliberately peels only a few known layers.
 */
export const extractChatInquiryPanel = (value: unknown): ChatInquiryPanel | null => {
  const visit = (candidate: unknown, depth: number, inheritedState = ''): ChatInquiryPanel | null => {
    if (!candidate || typeof candidate !== 'object' || Array.isArray(candidate) || depth > 6) return null;
    const record = candidate as Record<string, unknown>;
    // Tool/item lifecycle completion means the inquiry was opened. It must
    // never be mistaken for the user's answer when unwrapping durable rows.
    const lifecycle = ['kind', 'item_id', 'event_type', 'tool', 'tool_call_id'].some(key => key in record);
    const carrierState = String(record.state ?? '').trim().toLowerCase();
    const direct = record.questions;
    if (Array.isArray(direct) && direct.length > 0) {
      const state = carrierState || String((!lifecycle ? record.status : '') || inheritedState).trim().toLowerCase();
      const status: ChatInquiryPanel['status'] =
        state === 'answered' || state === 'completed' ? 'answered' :
          ['dismissed', 'cancelled', 'canceled'].includes(state) ? 'dismissed' : 'pending';
      return {
        ...(record as unknown as ChatInquiryPanel),
        questions: direct as ChatInquiryPanel['questions'],
        status,
        keepOpen: typeof (record.keepOpen ?? record.keep_open) === 'boolean'
          ? Boolean(record.keepOpen ?? record.keep_open)
          : record.awaiting !== false
      };
    }
    for (const key of ['panel', 'question_panel', 'questionPanel', 'data', 'result', 'payload', 'meta']) {
      const nested = visit(record[key], depth + 1, carrierState || inheritedState);
      if (nested) return nested;
    }
    return null;
  };
  return visit(value, 0);
};

export const presentChatInquiry = (panel: ChatInquiryPanel | null | undefined, status: string): ChatInquiryPanel | null => {
  if (!panel?.questions?.length) return null;
  const dismissed = ['cancelled', 'failed', 'rejected', 'stopped', 'interrupted'].includes(status);
  return { ...panel, keepOpen: panel.keepOpen ?? panel.keep_open ?? true,
    status: dismissed ? 'dismissed' : status === 'completed' ? 'answered' : panel.status };
};
