const ensureFallbackTextarea = () => {
  if (typeof document === 'undefined') return null;
  const existing = document.getElementById('wunder-clipboard-helper');
  if (existing) return existing;
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

export const copyText = async (rawText) => {
  const text = String(rawText ?? '');
  if (!text.trim()) return false;

  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch (error) {
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
  } catch (error) {
    return false;
  }
};
