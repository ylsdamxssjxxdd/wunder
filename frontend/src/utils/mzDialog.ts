import { reactive } from 'vue';

/**
 * 蜂巢主题弹窗队列（替代 ElMessageBox）。
 *
 * ElMessageBox 渲染的是 Element Plus 自带盒子（蓝色主按钮、方角、系统字体），
 * 与蜂巢的暖色赤陶配色和圆角语言不一致。这里用 Vue 组件渲染同样语义的
 * confirm / prompt 弹窗，样式走 `--ui-accent` 色板，视觉与 el-dialog 派生的
 * 业务弹窗保持一致。
 *
 * 用法与 ElMessageBox 对齐，只是不再靠 reject 表示取消：
 *   const ok = await mzConfirm({ title, message });
 *   const name = await mzPrompt({ title, inputValue });   // null = 取消
 *   const { action } = await mzDialog({ ... });           // 'confirm' | 'cancel' | 'close'
 */

export type MzDialogKind = 'confirm' | 'prompt';
export type MzDialogTone = 'default' | 'warning' | 'danger';
export type MzDialogAction = 'confirm' | 'cancel' | 'close';
export type MzDialogResult = { action: MzDialogAction; value: string };
export type MzDialogValidator = (value: string) => true | string;

export type MzDialogOptions = {
  kind?: MzDialogKind;
  title?: string;
  message?: string;
  confirmText?: string;
  cancelText?: string;
  /** danger：确认按钮转红，用于删除类确认。 */
  tone?: MzDialogTone;
  /** 只留确认按钮（提示语义）。 */
  alertOnly?: boolean;
  // prompt 专用
  inputValue?: string;
  placeholder?: string;
  label?: string;
  multiline?: boolean;
  maxlength?: number;
  selectOnFocus?: boolean;
  /** 返回 true 通过，返回字符串＝展示在输入框下方的错误提示。 */
  validator?: MzDialogValidator;
};

export type MzDialogEntry = {
  id: number;
  kind: MzDialogKind;
  title: string;
  message: string;
  confirmText: string;
  cancelText: string;
  tone: MzDialogTone;
  alertOnly: boolean;
  inputValue: string;
  placeholder: string;
  label: string;
  multiline: boolean;
  maxlength: number;
  selectOnFocus: boolean;
  validator: MzDialogValidator | null;
  /** 用户输入草稿与校验提示：只在本组件内读写，不回流到调用方。 */
  value: string;
  error: string;
  resolve: (result: MzDialogResult) => void;
};

export const mzDialogQueue = reactive<MzDialogEntry[]>([]);

let sequence = 0;

export const settleMzDialog = (id: number, result: MzDialogResult): void => {
  const index = mzDialogQueue.findIndex((entry) => entry.id === id);
  if (index < 0) return;
  const [entry] = mzDialogQueue.splice(index, 1);
  entry.resolve(result);
};

export const mzDialog = (options: MzDialogOptions = {}): Promise<MzDialogResult> =>
  new Promise<MzDialogResult>((resolve) => {
    mzDialogQueue.push({
      id: (sequence += 1),
      kind: options.kind === 'prompt' ? 'prompt' : 'confirm',
      title: options.title || '',
      message: options.message || '',
      confirmText: options.confirmText || '',
      cancelText: options.cancelText || '',
      tone: options.tone === 'danger' ? 'danger' : options.tone === 'warning' ? 'warning' : 'default',
      alertOnly: options.alertOnly === true,
      inputValue: options.inputValue ?? '',
      placeholder: options.placeholder || '',
      label: options.label || '',
      multiline: options.multiline === true,
      maxlength: Number(options.maxlength) > 0 ? Number(options.maxlength) : 0,
      selectOnFocus: options.selectOnFocus !== false,
      validator: typeof options.validator === 'function' ? options.validator : null,
      value: options.inputValue ?? '',
      error: '',
      resolve
    });
  });

export const mzConfirm = async (options: MzDialogOptions = {}): Promise<boolean> => {
  const result = await mzDialog({ ...options, kind: 'confirm' });
  return result.action === 'confirm';
};

export const mzPrompt = async (options: MzDialogOptions = {}): Promise<string | null> => {
  const result = await mzDialog({ ...options, kind: 'prompt' });
  return result.action === 'confirm' ? result.value : null;
};
