import { mzConfirm, type MzDialogOptions } from '@/utils/mzDialog';

/**
 * 统一的确认弹窗：走蜂巢主题弹窗（mzConfirm），取消一律返回 false。
 *
 * 历史调用传的是 ElMessageBox 风格的第三参（type / confirmButtonText /
 * cancelButtonText），这里做一层翻译，避免调用方因换组件丢掉按钮文案与语义：
 * `type: 'warning' | 'error'` → 主题弹窗的 warning / danger 色调。
 */
type LegacyMessageBoxOptions = MzDialogOptions & {
  type?: 'warning' | 'info' | 'success' | 'error' | 'question';
  confirmButtonText?: string;
  cancelButtonText?: string;
};

export const confirmWithFallback = async (
  message: string,
  title: string,
  options: LegacyMessageBoxOptions = {}
): Promise<boolean> =>
  mzConfirm({
    title,
    message,
    tone:
      options.tone ??
      (options.type === 'error' ? 'danger' : options.type === 'warning' ? 'warning' : 'default'),
    confirmText: options.confirmText || options.confirmButtonText || '',
    cancelText: options.cancelText || options.cancelButtonText || ''
  });
