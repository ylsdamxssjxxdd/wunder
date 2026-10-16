/** Bounded interactive metadata carried by the durable assistant item. */
export interface ChatInquiryOption { label: string; description: string; recommended: boolean }
export interface ChatInquiryQuestion { question: string; multiple: boolean; options: ChatInquiryOption[] }
export interface ChatInquiryPanel {
  questions: ChatInquiryQuestion[];
  keep_open?: boolean;
  keepOpen?: boolean;
  status: 'pending' | 'answered' | 'dismissed';
}

export const presentChatInquiry = (panel: ChatInquiryPanel | null | undefined, status: string): ChatInquiryPanel | null => {
  if (!panel?.questions?.length) return null;
  const dismissed = ['cancelled', 'failed', 'rejected', 'stopped', 'interrupted'].includes(status);
  return { ...panel, keepOpen: panel.keepOpen ?? panel.keep_open ?? true,
    status: dismissed ? 'dismissed' : status === 'completed' ? 'answered' : panel.status };
};
