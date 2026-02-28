import { ElMessageBox, type ElMessageBoxOptions } from 'element-plus';

import { isDesktopModeEnabled } from '@/config/desktop';

const isCancelLikeError = (error: unknown): boolean => {
  const action =
    typeof error === 'string'
      ? error
      : typeof (error as { action?: unknown })?.action === 'string'
        ? String((error as { action?: unknown }).action)
        : '';
  return action === 'cancel' || action === 'close';
};

export const confirmWithFallback = async (
  message: string,
  title: string,
  options: ElMessageBoxOptions = {}
): Promise<boolean> => {
  try {
    await ElMessageBox.confirm(message, title, options);
    return true;
  } catch (error) {
    if (isCancelLikeError(error)) {
      return false;
    }
    if (isDesktopModeEnabled() && typeof window !== 'undefined' && typeof window.confirm === 'function') {
      return window.confirm(`${title}\n\n${message}`);
    }
    return false;
  }
};
