export const WORKSPACE_RESOURCE_LOADING_LABEL_DELAY_MS = 160;

export const resolveWorkspaceLoadingLabel = (
  status: HTMLElement | null,
  fallbackLabel: string
): string => {
  const raw = status?.dataset?.loadingLabel;
  const normalized = String(raw || '').trim();
  return normalized || fallbackLabel;
};

export const scheduleWorkspaceLoadingLabel = (
  card: HTMLElement,
  status: HTMLElement | null,
  fallbackLabel: string,
  delayMs: number = WORKSPACE_RESOURCE_LOADING_LABEL_DELAY_MS
): number | null => {
  if (!status || typeof window === 'undefined') return null;
  status.textContent = '';
  const label = resolveWorkspaceLoadingLabel(status, fallbackLabel);
  return window.setTimeout(() => {
    if (!card.isConnected || card.dataset.workspaceState !== 'loading') return;
    status.textContent = label;
  }, delayMs);
};

export const clearWorkspaceLoadingLabelTimer = (timerId: number | null) => {
  if (timerId === null || typeof window === 'undefined') return;
  window.clearTimeout(timerId);
};

export const getFilenameFromHeaders = (
  headers: Record<string, unknown> | undefined,
  fallback: string
): string => {
  const disposition = String(headers?.['content-disposition'] || headers?.['Content-Disposition'] || '').trim();
  if (!disposition) return fallback;
  const utf8Match = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
  if (utf8Match?.[1]) {
    try {
      return decodeURIComponent(utf8Match[1]);
    } catch {
      return utf8Match[1];
    }
  }
  const match = /filename="?([^";]+)"?/i.exec(disposition);
  return match?.[1] || fallback;
};

const getFileExtension = (filename: string): string => {
  const base = String(filename || '').split('?')[0].split('#')[0];
  const parts = base.split('.');
  if (parts.length < 2) return '';
  return String(parts.pop() || '').toLowerCase();
};

const IMAGE_MIME_BY_EXTENSION: Record<string, string> = {
  bmp: 'image/bmp',
  gif: 'image/gif',
  jpeg: 'image/jpeg',
  jpg: 'image/jpeg',
  png: 'image/png',
  svg: 'image/svg+xml',
  webp: 'image/webp'
};

const WORKSPACE_RESOURCE_DIAGNOSTIC_BODY_LIMIT = 260;

export type WorkspaceResourceErrorDiagnostics = {
  status?: number | null;
  code?: string;
  contentType?: string;
  size?: number | null;
  message?: string;
  bodySnippet?: string;
};

type WorkspaceResourceErrorLike = Error & {
  response?: {
    status?: unknown;
    headers?: Record<string, unknown>;
    data?: unknown;
  };
  workspaceResourceDiagnostics?: WorkspaceResourceErrorDiagnostics;
};

type WorkspaceResourceResponseLike = {
  status?: unknown;
  headers?: Record<string, unknown>;
  data?: unknown;
};

const shouldRetypeImageBlob = (blob: Blob, contentType: string, expectedType: string): boolean => {
  if (!expectedType || blob.type === expectedType) return false;
  const rawType = String(blob.type || contentType || '').trim().toLowerCase();
  return !rawType || rawType === 'application/octet-stream' || rawType === 'binary/octet-stream';
};

type WorkspaceImagePreviewElement = HTMLImageElement & {
  __wunderImageDecoder?: HTMLImageElement;
};

const clearWorkspaceImageDecoder = (preview: WorkspaceImagePreviewElement) => {
  const decoder = preview.__wunderImageDecoder;
  if (!decoder) return;
  decoder.onload = null;
  decoder.onerror = null;
  delete preview.__wunderImageDecoder;
};

const getHeaderValue = (
  headers: Record<string, unknown> | undefined,
  name: string
): string => {
  if (!headers) return '';
  const lowerName = name.toLowerCase();
  for (const [key, value] of Object.entries(headers)) {
    if (key.toLowerCase() === lowerName) {
      return String(value || '').trim();
    }
  }
  return '';
};

const normalizeStatus = (value: unknown): number | null => {
  const status = Number(value);
  return Number.isFinite(status) && status > 0 ? status : null;
};

const isTextualContentType = (contentType: string): boolean => {
  const normalized = String(contentType || '').toLowerCase();
  return (
    normalized.startsWith('text/') ||
    normalized.includes('json') ||
    normalized.includes('xml') ||
    normalized.includes('html') ||
    normalized.includes('problem+')
  );
};

const readBlobSnippet = async (blob: Blob, contentType: string): Promise<string> => {
  if (!(blob instanceof Blob) || blob.size <= 0 || !isTextualContentType(contentType)) {
    return '';
  }
  try {
    const text = await blob.text();
    return text.replace(/\s+/g, ' ').trim().slice(0, WORKSPACE_RESOURCE_DIAGNOSTIC_BODY_LIMIT);
  } catch {
    return '';
  }
};

const resolvePayloadMessage = (payload: unknown): { code: string; message: string } => {
  if (!payload || typeof payload !== 'object') return { code: '', message: '' };
  const record = payload as Record<string, unknown>;
  const error = record.error && typeof record.error === 'object'
    ? (record.error as Record<string, unknown>)
    : {};
  const detail = record.detail && typeof record.detail === 'object'
    ? (record.detail as Record<string, unknown>)
    : {};
  return {
    code: String(error.code || detail.code || record.code || '').trim(),
    message: String(
      error.message ||
        detail.message ||
        record.message ||
        record.error_message ||
        (typeof record.detail === 'string' ? record.detail : '') ||
        ''
    ).trim()
  };
};

export const buildWorkspaceResourceErrorDiagnostics = async (
  response: WorkspaceResourceResponseLike | undefined,
  fallbackContentType = ''
): Promise<WorkspaceResourceErrorDiagnostics> => {
  const headers = response?.headers;
  const data = response?.data;
  const blob = data instanceof Blob ? data : null;
  const contentType = String(
    getHeaderValue(headers, 'content-type') ||
      blob?.type ||
      fallbackContentType ||
      ''
  ).trim();
  const diagnostics: WorkspaceResourceErrorDiagnostics = {
    status: normalizeStatus(response?.status),
    contentType,
    size: blob ? blob.size : null
  };
  if (typeof data === 'string') {
    diagnostics.bodySnippet = data.replace(/\s+/g, ' ').trim().slice(0, WORKSPACE_RESOURCE_DIAGNOSTIC_BODY_LIMIT);
  } else if (blob) {
    diagnostics.bodySnippet = await readBlobSnippet(blob, contentType);
  } else {
    const payload = resolvePayloadMessage(data);
    diagnostics.code = payload.code;
    diagnostics.message = payload.message;
  }
  if (diagnostics.bodySnippet && !diagnostics.message) {
    try {
      const payload = resolvePayloadMessage(JSON.parse(diagnostics.bodySnippet));
      diagnostics.code = payload.code;
      diagnostics.message = payload.message;
    } catch {
      // Keep the raw snippet for non-JSON error bodies.
    }
  }
  return diagnostics;
};

export const resolveWorkspaceResourceErrorDiagnostics = (
  error: unknown
): WorkspaceResourceErrorDiagnostics | undefined => {
  if (!error || typeof error !== 'object') return undefined;
  const source = error as WorkspaceResourceErrorLike;
  return source.workspaceResourceDiagnostics;
};

export const hydrateWorkspaceResourceErrorDiagnostics = async (
  error: unknown,
  fallbackContentType = ''
): Promise<unknown> => {
  if (!error || typeof error !== 'object') return error;
  const source = error as WorkspaceResourceErrorLike;
  if (source.workspaceResourceDiagnostics) return source;
  source.workspaceResourceDiagnostics = await buildWorkspaceResourceErrorDiagnostics(
    source.response,
    fallbackContentType
  );
  return source;
};

const formatWorkspaceResourceDiagnostics = (
  diagnostics: WorkspaceResourceErrorDiagnostics | undefined
): string => {
  if (!diagnostics) return '';
  const parts = [
    diagnostics.status ? `HTTP ${diagnostics.status}` : '',
    diagnostics.code || '',
    diagnostics.contentType ? `type=${diagnostics.contentType}` : '',
    diagnostics.size !== null && diagnostics.size !== undefined ? `size=${diagnostics.size}` : '',
    diagnostics.message || '',
    diagnostics.bodySnippet || ''
  ].filter(Boolean);
  return parts.join(' | ');
};

const clearWorkspaceResourceDiagnostics = (card: HTMLElement, status: HTMLElement | null) => {
  delete card.dataset.workspaceErrorStatus;
  delete card.dataset.workspaceErrorCode;
  delete card.dataset.workspaceErrorContentType;
  delete card.dataset.workspaceErrorSize;
  delete card.dataset.workspaceErrorDetail;
  if (card.title) card.removeAttribute('title');
  if (status?.title) status.removeAttribute('title');
};

const applyWorkspaceResourceDiagnostics = (
  card: HTMLElement,
  status: HTMLElement | null,
  diagnostics: WorkspaceResourceErrorDiagnostics | undefined,
  message: string
) => {
  clearWorkspaceResourceDiagnostics(card, status);
  if (!diagnostics) return;
  if (diagnostics.status) card.dataset.workspaceErrorStatus = String(diagnostics.status);
  if (diagnostics.code) card.dataset.workspaceErrorCode = diagnostics.code;
  if (diagnostics.contentType) card.dataset.workspaceErrorContentType = diagnostics.contentType;
  if (diagnostics.size !== null && diagnostics.size !== undefined) {
    card.dataset.workspaceErrorSize = String(diagnostics.size);
  }
  const detail = formatWorkspaceResourceDiagnostics(diagnostics);
  if (!detail) return;
  card.dataset.workspaceErrorDetail = detail;
  card.title = `${message}\n${detail}`;
  if (status) status.title = detail;
};

export const normalizeWorkspaceImageBlob = (
  blob: Blob,
  filename: string,
  contentType: string
): Blob => {
  if (!(blob instanceof Blob)) return blob;
  const expectedType = IMAGE_MIME_BY_EXTENSION[getFileExtension(filename)] || '';
  if (!expectedType || blob.type === expectedType) return blob;
  const headerType = String(contentType || '').toLowerCase();
  if (headerType.startsWith('image/')) {
    return blob.slice(0, blob.size, expectedType);
  }
  return shouldRetypeImageBlob(blob, contentType, expectedType)
    ? blob.slice(0, blob.size, expectedType)
    : blob;
};

export const isWorkspaceImageBlobLikelyInvalid = (
  blob: Blob,
  filename: string,
  contentType: string
): boolean => {
  if (!(blob instanceof Blob)) return true;
  const expectedType = IMAGE_MIME_BY_EXTENSION[getFileExtension(filename)] || '';
  if (!expectedType) return false;
  if (blob.size <= 0) return true;
  const rawType = String(blob.type || contentType || '').trim().toLowerCase();
  if (!rawType || rawType === expectedType || rawType.startsWith('image/')) return false;
  return rawType !== 'application/octet-stream' && rawType !== 'binary/octet-stream';
};

export const normalizeWorkspaceImageResponseBlob = async (
  blob: Blob,
  filename: string,
  contentType: string,
  response?: WorkspaceResourceResponseLike
): Promise<Blob> => {
  if (isWorkspaceImageBlobLikelyInvalid(blob, filename, contentType)) {
    const error = new Error('workspace image response is not a decodable image') as WorkspaceResourceErrorLike;
    error.response = response;
    error.workspaceResourceDiagnostics = await buildWorkspaceResourceErrorDiagnostics(
      response || { data: blob },
      contentType
    );
    throw error;
  }
  return normalizeWorkspaceImageBlob(blob, filename, contentType);
};

export const markWorkspaceImageCardReady = (
  card: HTMLElement,
  status: HTMLElement | null,
  loadingTimerId: number | null
) => {
  clearWorkspaceLoadingLabelTimer(loadingTimerId);
  clearWorkspaceResourceDiagnostics(card, status);
  card.dataset.workspaceState = 'ready';
  card.classList.remove('is-error');
  card.classList.add('is-ready');
  if (status) status.textContent = '';
};

export const markWorkspaceImageCardError = (
  card: HTMLElement,
  status: HTMLElement | null,
  loadingTimerId: number | null,
  message: string,
  diagnostics?: WorkspaceResourceErrorDiagnostics
) => {
  clearWorkspaceLoadingLabelTimer(loadingTimerId);
  card.dataset.workspaceState = 'error';
  card.classList.remove('is-ready');
  card.classList.add('is-error');
  if (status) status.textContent = message;
  applyWorkspaceResourceDiagnostics(card, status, diagnostics, message);
};

export const bindWorkspaceImagePreviewState = (
  card: HTMLElement,
  preview: HTMLImageElement,
  objectUrl: string,
  options: {
    status?: HTMLElement | null;
    loadingTimerId?: number | null;
    failedLabel: string;
    onDecodeError?: () => void;
  }
) => {
  const status = options.status ?? null;
  const loadingTimerId = options.loadingTimerId ?? null;
  const failedLabel = options.failedLabel;
  let settled = false;
  clearWorkspaceImageDecoder(preview as WorkspaceImagePreviewElement);
  const decoder = new Image();
  (preview as WorkspaceImagePreviewElement).__wunderImageDecoder = decoder;
  const cleanup = () => {
    preview.onload = null;
    preview.onerror = null;
    decoder.onload = null;
    decoder.onerror = null;
    if ((preview as WorkspaceImagePreviewElement).__wunderImageDecoder === decoder) {
      delete (preview as WorkspaceImagePreviewElement).__wunderImageDecoder;
    }
  };
  const ready = () => {
    if (settled) return;
    settled = true;
    cleanup();
    preview.src = objectUrl;
    markWorkspaceImageCardReady(card, status, loadingTimerId);
  };
  const failed = () => {
    if (settled) return;
    settled = true;
    cleanup();
    options.onDecodeError?.();
    markWorkspaceImageCardError(card, status, loadingTimerId, failedLabel);
  };

  preview.removeAttribute('src');
  decoder.onload = ready;
  decoder.onerror = failed;
  decoder.src = objectUrl;
};

export const saveObjectUrlAsFile = (url: string, filename: string) => {
  const link = document.createElement('a');
  link.href = url;
  link.download = filename || 'download';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
};

export const resetWorkspaceImageCardState = (
  card: HTMLElement,
  options: {
    clearSrc?: boolean;
    includeReady?: boolean;
  } = {}
) => {
  const kind = String(card.dataset.workspaceKind || 'image').trim().toLowerCase();
  if (kind !== 'image') return false;
  const state = String(card.dataset.workspaceState || '').trim().toLowerCase();
  if (!options.includeReady && state === 'ready') return false;
  card.dataset.workspaceState = '';
  card.classList.remove('is-error');
  card.classList.remove('is-ready');
  const preview = card.querySelector('.ai-resource-preview');
  if (preview instanceof HTMLImageElement) {
    preview.onload = null;
    preview.onerror = null;
    clearWorkspaceImageDecoder(preview as WorkspaceImagePreviewElement);
    if (options.clearSrc) {
      preview.removeAttribute('src');
    }
  }
  const status = card.querySelector('.ai-resource-status');
  if (status instanceof HTMLElement) {
    status.textContent = '';
    clearWorkspaceResourceDiagnostics(card, status);
  } else {
    clearWorkspaceResourceDiagnostics(card, null);
  }
  return true;
};

export const resetWorkspaceImageCards = (
  container: ParentNode | null | undefined,
  options: {
    clearSrc?: boolean;
    includeReady?: boolean;
  } = {}
) => {
  if (!container || typeof container.querySelectorAll !== 'function') return;
  container.querySelectorAll('.ai-resource-card[data-workspace-path]').forEach((node) => {
    resetWorkspaceImageCardState(node as HTMLElement, options);
  });
};
