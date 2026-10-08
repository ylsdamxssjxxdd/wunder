import { resolveApiError } from '@/utils/apiError';

type BlobApiErrorLike = {
  response?: {
    data?: unknown;
  };
};

const isJsonContentType = (blob: Blob | null | undefined): boolean => {
  const type = String(blob?.type || '').toLowerCase();
  return type.includes('application/json') || type.includes('text/json');
};

const tryParseJsonMessage = (payload: string): string => {
  try {
    const parsed = JSON.parse(payload) as Record<string, unknown>;
    return resolveApiError({ response: { data: parsed } }, '').message;
  } catch {
    return '';
  }
};

export const resolveBlobApiErrorMessage = async (
  error: unknown,
  fallback: string
): Promise<string> => {
  const direct = resolveApiError(error, '').message;
  const blob = (error as BlobApiErrorLike)?.response?.data;
  if (!(blob instanceof Blob) || !isJsonContentType(blob)) {
    return direct || fallback;
  }
  try {
    const text = await blob.text();
    const parsed = tryParseJsonMessage(text);
    if (parsed) {
      return parsed;
    }
    const trimmed = text.trim();
    return trimmed || direct || fallback;
  } catch {
    return direct || fallback;
  }
};
