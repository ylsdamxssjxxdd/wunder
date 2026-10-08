const IMAGE_FILE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp']);

export const MAX_CHAT_REQUEST_TEXT_INPUT_CHARS = 1 << 20;

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

const readStringField = (source: Record<string, unknown>, keys: string[]): string => {
  for (const key of keys) {
    const value = source[key];
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
};

const attachmentIsImage = (attachment: unknown, content: string): boolean => {
  const record = asRecord(attachment);
  const contentType = readStringField(record, ['content_type', 'contentType', 'mime_type', 'mimeType']).toLowerCase();
  if (contentType.startsWith('image') || contentType.includes('image')) {
    return true;
  }
  if (content.startsWith('data:image/')) {
    return true;
  }
  const name = readStringField(record, ['name', 'filename']).toLowerCase();
  const parts = name.split('.');
  const extension = parts.length > 1 ? parts[parts.length - 1] : '';
  return IMAGE_FILE_EXTENSIONS.has(extension);
};

export const measureChatRequestTextInputChars = (content: unknown, attachments: unknown[] = []): number => {
  let total = Array.from(String(content || '').trim()).length;
  attachments.forEach((attachment) => {
    const record = asRecord(attachment);
    const textContent = readStringField(record, ['content', 'text']);
    if (!textContent || attachmentIsImage(record, textContent)) {
      return;
    }
    total += Array.from(textContent).length;
  });
  return total;
};

export type ChatRequestTextInputOverflow = {
  actualChars: number;
  maxChars: number;
  message: string;
};

export const resolveChatRequestTextInputOverflow = (
  content: unknown,
  attachments: unknown[] = [],
  formatMessage: (params: { actualChars: number; maxChars: number }) => string
): ChatRequestTextInputOverflow | null => {
  const actualChars = measureChatRequestTextInputChars(content, attachments);
  if (actualChars <= MAX_CHAT_REQUEST_TEXT_INPUT_CHARS) {
    return null;
  }
  return {
    actualChars,
    maxChars: MAX_CHAT_REQUEST_TEXT_INPUT_CHARS,
    message: formatMessage({
      actualChars,
      maxChars: MAX_CHAT_REQUEST_TEXT_INPUT_CHARS
    })
  };
};

export const buildChatRequestTextInputOverflowError = (overflow: ChatRequestTextInputOverflow): Error => {
  const error = new Error(overflow.message) as Error & {
    response?: {
      status: number;
      data: Record<string, unknown>;
    };
  };
  error.response = {
    status: 400,
    data: {
      code: 'INVALID_REQUEST',
      message: overflow.message,
      detail: {
        field: 'input_text',
        max_chars: overflow.maxChars,
        actual_chars: overflow.actualChars,
        message: overflow.message
      }
    }
  };
  return error;
};
