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

export const normalizeWorkspaceImageBlob = (
  blob: Blob,
  filename: string,
  contentType: string
): Blob => {
  if (!(blob instanceof Blob)) return blob;
  if (getFileExtension(filename) !== 'svg') return blob;
  const expectedType = 'image/svg+xml';
  if (blob.type === expectedType) return blob;
  const headerType = String(contentType || '').toLowerCase();
  if (headerType.includes('image/svg')) {
    return blob.slice(0, blob.size, expectedType);
  }
  return blob.slice(0, blob.size, expectedType);
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
    if (options.clearSrc) {
      preview.removeAttribute('src');
    }
  }
  const status = card.querySelector('.ai-resource-status');
  if (status instanceof HTMLElement) {
    status.textContent = '';
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
