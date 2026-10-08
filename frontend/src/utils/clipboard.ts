type DesktopClipboardBridge = {
  copyText?: (text: string) => Promise<boolean> | boolean;
};

const resolveDesktopClipboardBridge = (): DesktopClipboardBridge | null => {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopClipboardBridge }).wunderDesktop;
  if (candidate && typeof candidate.copyText === 'function') {
    return candidate;
  }
  return null;
};

const ensureFallbackTextarea = (): HTMLTextAreaElement | null => {
  if (typeof document === 'undefined') return null;
  const existing = document.getElementById('wunder-clipboard-helper');
  if (existing instanceof HTMLTextAreaElement) return existing;
  const textarea = document.createElement('textarea');
  textarea.id = 'wunder-clipboard-helper';
  textarea.setAttribute('readonly', '');
  textarea.setAttribute('aria-hidden', 'true');
  textarea.style.position = 'fixed';
  textarea.style.top = '-1000px';
  textarea.style.left = '-1000px';
  textarea.style.opacity = '0';
  textarea.style.pointerEvents = 'none';
  document.body.appendChild(textarea);
  return textarea;
};

export const copyText = async (rawText: unknown): Promise<boolean> => {
  const text = String(rawText ?? '');
  if (!text.trim()) return false;

  const desktopBridge = resolveDesktopClipboardBridge();
  if (desktopBridge?.copyText) {
    try {
      const copied = await desktopBridge.copyText(text);
      if (copied !== false) {
        return true;
      }
    } catch {
      // Continue to browser clipboard fallbacks.
    }
  }

  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // Fallback to execCommand for older/blocked clipboard API.
    }
  }

  const textarea = ensureFallbackTextarea();
  if (!textarea) return false;
  textarea.value = text;
  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, textarea.value.length);
  try {
    return document.execCommand('copy');
  } catch {
    return false;
  }
};
